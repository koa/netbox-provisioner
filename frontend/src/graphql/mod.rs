use crate::error::FrontendError;
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::LazyLock;
use yew::Component;
use yew::html::Scope;
use yew_oauth2::prelude::{Authentication, OAuth2Context};

pub mod anonymous;
pub mod authenticated;
pub mod history;
pub mod live;
pub mod schema;
pub mod types;

static GRAPHQL_AUTHENTICATED_URL: LazyLock<String> =
    LazyLock::new(|| format!("{}/graphql", host()));
static GRAPHQL_ANONYMOUS_URL: LazyLock<String> =
    LazyLock::new(|| format!("{}/graphql_anonymous", host()));

pub fn host() -> String {
    let location = web_sys::window().unwrap().location();
    let host = location.host().unwrap();
    let protocol = location.protocol().unwrap();
    format!("{protocol}//{host}")
}

/// Send Graphql-Query to server
pub async fn query_authenticated<Q: GraphQLQuery, S: Component>(
    scope: Scope<S>,
    request: Q::Variables,
) -> Result<Q::ResponseData, FrontendError> {
    let response = query_authenticated_response::<Q, S>(scope, request).await?;
    if let Some(data) = response.data {
        Ok(data)
    } else {
        Err(FrontendError::Graphql(response.errors.unwrap_or_default()))
    }
}

pub async fn query_authenticated_response<Q: GraphQLQuery, S: Component>(
    scope: Scope<S>,
    request: Q::Variables,
) -> Result<Response<<Q as GraphQLQuery>::ResponseData>, FrontendError> {
    let client = authenticated_client(&scope)?;
    post(&client, GRAPHQL_AUTHENTICATED_URL.as_str(), &Q::build_query(request)).await
}

/// A query or mutation of cynic, sent as [`query_authenticated`] sends one: without data, the
/// errors.
pub async fn request<T: DeserializeOwned, V: Serialize, S: Component>(
    scope: Scope<S>,
    operation: cynic::Operation<T, V>,
) -> Result<T, FrontendError> {
    let client = authenticated_client(&scope)?;
    let response: Response<T> =
        post(&client, GRAPHQL_AUTHENTICATED_URL.as_str(), &operation).await?;
    response.data.ok_or_else(|| FrontendError::Graphql(response.errors.unwrap_or_default()))
}

/// A client sending the user's token.
fn authenticated_client<S: Component>(scope: &Scope<S>) -> Result<reqwest::Client, FrontendError> {
    let mut headers = HeaderMap::new();
    if let Some(token) = token(scope) {
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
    }
    Ok(reqwest::Client::builder().default_headers(headers).build()?)
}

/// The token the backend takes: the ID token, which carries the user's name (Pocket-ID's
/// access token only `sub`), else the access token (a refresh may come without a new ID token).
fn token<S: Component>(scope: &Scope<S>) -> Option<String> {
    match scope.context::<OAuth2Context>(Default::default()) {
        Some((OAuth2Context::Authenticated(Authentication { access_token, id_token, .. }), _)) => {
            Some(id_token.unwrap_or(access_token))
        }
        _ => None,
    }
}

/// Sends a query of graphql_client or cynic. A response without success (a token the server
/// refuses: `401` or `403` with a text) is that text, not a failure to decode it as JSON. With
/// errors, `data` may lack the fields that failed (async-graphql leaves out a non-null root
/// field with an error, `netboxPlan` refused with `PORT_ADDRESSED`): then it is only the
/// errors, not a failure to decode `data`.
async fn post<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &impl Serialize,
) -> Result<Response<T>, FrontendError> {
    let response = client.post(url).json(body).send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(FrontendError::Server { status: status.as_u16(), text });
    }
    let Response {
        data,
        errors,
        extensions,
    } = response.json::<Response<serde_json::Value>>().await?;
    let data = match data {
        Some(data) if errors.as_ref().is_some_and(|errors| !errors.is_empty()) => {
            serde_json::from_value(data).ok()
        }
        Some(data) => Some(serde_json::from_value(data)?),
        None => None,
    };
    Ok(Response {
        data,
        errors,
        extensions,
    })
}

pub async fn query_anonymous<Q: GraphQLQuery>(
    request: Q::Variables,
) -> Result<Q::ResponseData, FrontendError> {
    let client = reqwest::Client::builder().build()?;
    let response: Response<Q::ResponseData> =
        post(&client, GRAPHQL_ANONYMOUS_URL.as_str(), &Q::build_query(request)).await?;
    if let Some(data) = response.data {
        Ok(data)
    } else {
        Err(FrontendError::Graphql(response.errors.unwrap_or_default()))
    }
}
