use actix_4_jwt_auth::{
    DecodedInfo, OIDCValidationError, Oidc, OidcBiscuitValidator, OidcConfig,
    biscuit::{ClaimPresenceOptions, Presence, TemporalOptions, Validation, ValidationOptions},
};
use actix_web::{
    App, HttpResponse, HttpServer, get,
    body::{BoxBody, MessageBody},
    dev::{HttpServiceFactory, ServiceRequest, ServiceResponse},
    guard::Post,
    http::StatusCode,
    middleware::{Logger, Next, from_fn},
    web::{Data, resource},
};
use actix_web_prometheus::PrometheusMetricsBuilder;
use actix_web_static_files::{ResourceFiles, deps::static_files::Resource};
use async_graphql::futures_util::future::join_all;
use async_graphql_actix_web::{GraphQLRequest, GraphQLResponse};
use env_logger::Env;
use include_dir::{Dir, include_dir};
use log::{error, trace, warn};
use mime_db::lookup;
use prometheus::{HistogramVec, histogram_opts};
use std::{collections::HashMap, ffi::OsStr};
use thiserror::Error;
use tracing_actix_web::TracingLogger;

use backend::{
    config::CONFIG,
    context::UserInfo,
    graphql::{
        AnonymousGraphqlSchema, AuthenticatedGraphqlSchema, create_anonymous_schema, create_schema,
    },
    netbox::NetboxError,
    topology::TopologyHolder,
};

const BUILD_TIME: u64 = include!(concat!(env!("OUT_DIR"), "/timestamp.txt"));
static RESOURCE_FILES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist");

fn generate() -> HashMap<&'static str, Resource> {
    let mut map = HashMap::new();
    walk_resources(&mut map, &RESOURCE_FILES);
    map
}

fn walk_resources(target: &mut HashMap<&'static str, Resource>, dir: &'static Dir) {
    for file in dir.files() {
        let path = file.path();
        if let (Some(filename), Some(mime_type)) = (
            path.as_os_str().to_str(),
            path.extension()
                .and_then(OsStr::to_str)
                .and_then(|e| lookup(e)),
        ) {
            target.insert(
                filename,
                Resource {
                    data: file.contents(),
                    modified: BUILD_TIME,
                    mime_type,
                },
            );
        }
    }
    for directory in dir.dirs() {
        walk_resources(target, directory);
    }
}

async fn graphql(
    context: Data<ApplicationContext>,
    user: Option<DecodedInfo>,
    request: GraphQLRequest,
) -> GraphQLResponse {
    trace!("Execute Authenticated: {user:#?}");
    let schema = &context.schema;
    let histogram = context.graphql_request_histogram.clone();
    let request = request.into_inner();
    let subject = user.as_ref().and_then(|user| {
        user.payload
            .registered
            .subject
            .as_deref()
    });
    let found_user = if let Some(DecodedInfo { jwt: _jwt, payload }) = &user {
        match serde_json::from_value::<UserInfo>(payload.private.clone()) {
            Ok(user) => Some(user),
            Err(error) => {
                error!("Cannot decode user info: {:#?}: {error}", payload.private);
                None
            }
        }
    } else {
        None
    };
    let timer = histogram
        .with_label_values(&[
            request.operation_name.as_deref().unwrap_or_default(),
            found_user
                .as_ref()
                .and_then(|u| u.name.as_deref())
                .or(subject)
                .unwrap_or_default(),
        ])
        .start_timer();
    let request = if let Some(user) = found_user {
        request.data(user)
    } else {
        request
    };

    let response = schema.execute(request).await;
    timer.stop_and_record();
    response.into()
}
async fn graphql_anonymous(
    context: Data<ApplicationContext>,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let schema = &context.anonymous_schema;
    let histogram = context.graphql_request_histogram.clone();
    let request = request.into_inner();
    let timer = histogram
        .with_label_values(&[request.operation_name.as_deref().unwrap_or_default(), ""])
        .start_timer();

    let response = schema.execute(request).await;
    timer.stop_and_record();
    response.into()
}

#[get("/health")]
async fn health() -> &'static str {
    "Ok"
}

#[derive(Clone)]
struct ApplicationContext {
    graphql_request_histogram: HistogramVec,
    schema: AuthenticatedGraphqlSchema,
    anonymous_schema: AnonymousGraphqlSchema,
}

#[derive(Error, Debug)]
enum BackendError {
    #[error("An IO Error happened {0}")]
    IO(#[from] std::io::Error),
    #[error("An Error from prometheus {0}")]
    Prometheus(#[from] prometheus::Error),
    #[error("An Error from prometheus {0}")]
    ActixWebPrometheus(#[from] actix_web_prometheus::error::Error),
    #[error("Error on OIDC Validation {0}")]
    OidcValidationError(#[from] OIDCValidationError),
    #[error("Error from netbox {0}")]
    Netbox(#[from] NetboxError),
    #[error("State directory: {0}")]
    StateDir(#[from] backend::saved_job::StoreError),
}

#[actix_web::main]
async fn main() -> Result<(), BackendError> {
    env_logger::init_from_env(Env::default().filter_or("LOG_LEVEL", "debug"));

    let bind_addr = CONFIG.server_bind_address();
    let api_port = CONFIG.server_port();
    let mgmt_port = CONFIG.server_mgmt_port();

    let mut labels = HashMap::new();
    labels.insert("server".to_string(), "api".to_string());

    let graphql_request_histogram = HistogramVec::new(
        histogram_opts!("graphql_request", "Measure graphql queries"),
        &["name", "user"],
    )?;
    let prometheus = PrometheusMetricsBuilder::new("")
        .const_labels(labels)
        .build()?;

    let registry = prometheus.registry.clone();
    registry.register(Box::new(graphql_request_histogram.clone()))?;
    // before anything else: a second provisioner on the same state_dir stops here
    backend::saved_job::open(CONFIG.state_dir())?;
    let topology = TopologyHolder::default();
    // ensure initial fetch
    topology.fetch().await?;

    let schema = create_schema(topology.clone());
    let anonymous_schema = create_anonymous_schema();

    let issuer = CONFIG.auth_issuer().to_string();
    //info!("Issuer: {issuer}");
    let oidc = Oidc::new(OidcConfig::Issuer(issuer.into())).await?;

    let biscuit_validator = OidcBiscuitValidator {
        options: validation_options(),
    };

    let data = Data::new(ApplicationContext {
        graphql_request_histogram,
        schema,
        anonymous_schema,
    });
    let main_server = HttpServer::new(move || {
        let resources = generate();

        App::new()
            .wrap(prometheus.clone())
            .wrap(TracingLogger::default())
            .wrap(Logger::default())
            .app_data(data.clone())
            .app_data(oidc.clone())
            // `/…/`: workaround for proxy troubles
            .service(authenticated_graphql("/graphql", biscuit_validator.clone()))
            .service(authenticated_graphql("/graphql/", biscuit_validator.clone()))
            .service(anonymous_graphql("/graphql_anonymous"))
            .service(anonymous_graphql("/graphql_anonymous/"))
            .service(ResourceFiles::new("/", resources).resolve_not_found_to_root())
    })
    .bind((bind_addr, api_port))?
    .run();
    let mut labels = HashMap::new();
    labels.insert("server".to_string(), "mgmt".to_string());

    let prometheus = PrometheusMetricsBuilder::new("")
        .const_labels(labels)
        .registry(registry)
        .endpoint("/metrics")
        .build()
        .unwrap();
    let mgmt_server = HttpServer::new(move || App::new().wrap(prometheus.clone()).service(health))
        .bind((bind_addr, mgmt_port))?
        .workers(2)
        .run();
    if let Some(e) = join_all(vec![main_server, mgmt_server])
        .await
        .into_iter()
        .flat_map(|r| r.err())
        .next()
    {
        return Err(e.into());
    }
    Ok(())
}

/// The authenticated API: a valid token (`biscuit_validator`, why one is refused is logged by
/// `log_refused_token`) of a user in `auth_users` (`require_allowed_user`), otherwise `403`.
fn authenticated_graphql(
    path: &'static str,
    biscuit_validator: OidcBiscuitValidator,
) -> impl HttpServiceFactory {
    resource(path)
        .guard(Post())
        .wrap(from_fn(require_allowed_user))
        .wrap(biscuit_validator)
        .wrap(from_fn(log_refused_token))
        .to(graphql)
}

fn anonymous_graphql(path: &'static str) -> impl HttpServiceFactory {
    resource(path).guard(Post()).to(graphql_anonymous)
}

/// Lets only the users of `auth_users` (their `sub`) in, after the token is checked.
async fn require_allowed_user(
    mut req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let subject = req
        .extract::<DecodedInfo>()
        .await
        .ok()
        .and_then(|info| info.payload.registered.subject);
    if !subject
        .as_deref()
        .is_some_and(|subject| CONFIG.auth_users.iter().any(|id| **id == *subject))
    {
        warn!("user refused, not in auth_users: {subject:?}");
        return Ok(req.into_response(HttpResponse::Forbidden().body("user not in auth_users")));
    }
    Ok(next.call(req).await?.map_into_boxed_body())
}

/// What a token has to meet besides its signature. The audience must name this app, else
/// tokens the issuer made for other apps would pass. The times allow for clocks a little
/// apart: without, a token was refused as not yet valid by a server one second behind the
/// issuer (Pocket-ID).
fn validation_options() -> ValidationOptions {
    ValidationOptions {
        claim_presence_options: ClaimPresenceOptions {
            audience: Presence::Required,
            ..ClaimPresenceOptions::default()
        },
        issuer: Validation::Validate(CONFIG.auth_issuer().to_string()),
        audience: Validation::Validate(CONFIG.auth_client_id().to_string()),
        temporal_options: TemporalOptions {
            epsilon: chrono::Duration::seconds(60),
            now: None,
        },
        ..ValidationOptions::default()
    }
}

/// Logs why `OidcBiscuitValidator` refused a token with a valid signature, which it answers
/// only with `403` "Token does not have sufficient rights": the claims and the failed check
/// (`biscuit::ValidationError`, e.g. a token not yet valid by this server's clock).
async fn log_refused_token(
    mut req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let claims = req
        .extract::<DecodedInfo>()
        .await
        .ok()
        .map(|info| info.payload.registered);
    let response = next.call(req).await?;
    if response.status() == StatusCode::FORBIDDEN
        && let Some(claims) = claims
        && let Err(error) = claims.validate(validation_options())
    {
        warn!(
            "token refused: {error} (iss {:?}, aud {:?}, iat {:?}, nbf {:?}, exp {:?}, now {})",
            claims.issuer,
            claims.audience,
            claims.issued_at.map(|t| t.timestamp()),
            claims.not_before.map(|t| t.timestamp()),
            claims.expiry.map(|t| t.timestamp()),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|now| now.as_secs())
                .unwrap_or_default(),
        );
    }
    Ok(response)
}
