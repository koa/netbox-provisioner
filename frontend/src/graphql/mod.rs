use crate::{components::login_expired, error::FrontendError};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::LazyLock;
use yew::Component;
use yew::html::Scope;
use yew_oauth2::prelude::{Authentication, OAuth2Context};

/// An answer: the data, and the errors with their `extensions` (`code`, `params`,
/// `error::ServerProblem`).
pub type Response<T> = cynic::GraphQlResponse<T, serde_json::Value>;
/// An error of an answer.
pub type GraphqlError = cynic::GraphQlError<serde_json::Value>;

/// A segment of an error's `path` as text (`deviceById`, `0`).
pub fn path_segment(segment: &cynic::GraphQlErrorPathSegment) -> String {
    match segment {
        cynic::GraphQlErrorPathSegment::Field(field) => field.clone(),
        cynic::GraphQlErrorPathSegment::Index(index) => index.to_string(),
    }
}

pub mod anonymous;
pub mod authenticated;
pub mod live;

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

/// What cynic builds to send: a query or mutation, or a subscription, which the server answers
/// over HTTP with its first answer (`live::execute_once`).
pub trait Sendable: Serialize {
    type Data: DeserializeOwned + 'static;
}

impl<T: DeserializeOwned + 'static, V: Serialize> Sendable for cynic::Operation<T, V> {
    type Data = T;
}

impl<T: DeserializeOwned + 'static, V: Serialize> Sendable for cynic::StreamingOperation<T, V> {
    type Data = T;
}

/// Sends `operation` (cynic) to the authenticated endpoint: without data, the errors.
pub async fn request<O: Sendable, S: Component>(
    scope: Scope<S>,
    operation: O,
) -> Result<O::Data, FrontendError> {
    let response = request_response(scope, operation).await?;
    response
        .data
        .ok_or_else(|| FrontendError::Graphql(response.errors.unwrap_or_default()))
}

/// [`request`] giving data and errors both: a field that failed leaves the rest.
pub async fn request_response<O: Sendable, S: Component>(
    scope: Scope<S>,
    operation: O,
) -> Result<Response<O::Data>, FrontendError> {
    let client = authenticated_client(&scope)?;
    post(&client, GRAPHQL_AUTHENTICATED_URL.as_str(), &operation).await
}

/// A client sending the user's token.
fn authenticated_client<S: Component>(scope: &Scope<S>) -> Result<reqwest::Client, FrontendError> {
    let mut headers = HeaderMap::new();
    if let Some(token) = token(scope) {
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
    }
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

/// The token the backend takes: the ID token, which carries the user's name (Pocket-ID's
/// access token only `sub`), else the access token (a refresh may come without a new ID token).
fn token<S: Component>(scope: &Scope<S>) -> Option<String> {
    match scope.context::<OAuth2Context>(Default::default()) {
        Some((
            OAuth2Context::Authenticated(Authentication {
                access_token,
                id_token,
                ..
            }),
            _,
        )) => Some(id_token.unwrap_or(access_token)),
        _ => None,
    }
}

/// Sends a query. A response without success (a token the server
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
        if matches!(status.as_u16(), 401 | 403) && login_expired::refuses_token(&text) {
            login_expired::report(true);
        }
        return Err(FrontendError::Server {
            status: status.as_u16(),
            text,
        });
    }
    login_expired::report(false);
    let Response { data, errors } = response.json::<Response<serde_json::Value>>().await?;
    let data = match data {
        Some(data) if errors.as_ref().is_some_and(|errors| !errors.is_empty()) => {
            serde_json::from_value(data).ok()
        }
        Some(data) => Some(serde_json::from_value(data)?),
        None => None,
    };
    Ok(Response { data, errors })
}

/// Sends `operation` (cynic) to the anonymous endpoint, before the login.
pub async fn request_anonymous<O: Sendable>(operation: O) -> Result<O::Data, FrontendError> {
    let client = reqwest::Client::builder().build()?;
    let response: Response<O::Data> =
        post(&client, GRAPHQL_ANONYMOUS_URL.as_str(), &operation).await?;
    response
        .data
        .ok_or_else(|| FrontendError::Graphql(response.errors.unwrap_or_default()))
}
