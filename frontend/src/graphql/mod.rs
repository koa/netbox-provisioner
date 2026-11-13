//! Scope: Sending GraphQL requests and what all of them share; not the queries (`anonymous`,
//! `authenticated`).
use crate::{auth, error::FrontendError};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Serialize, de::DeserializeOwned};
use std::{sync::LazyLock, time::Duration};
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

/// How often a view that doesn't subscribe asks again (a site's diff job).
pub(crate) const POLL: Duration = Duration::from_secs(1);
/// How long to wait after the 1st, 2nd, … failed asking in a row; then the last one.
pub(crate) const RETRY: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

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
        if matches!(status.as_u16(), 401 | 403) && auth::refuses_token(&text) {
            auth::report(true);
        }
        return Err(FrontendError::Server {
            status: status.as_u16(),
            text,
        });
    }
    auth::report(false);
    Ok(decode(
        response.json::<Response<serde_json::Value>>().await?,
    )?)
}

/// The data of `response` as `T`; with errors `data` may lack the fields that failed
/// (async-graphql leaves out a non-null field with an error), then it is only the errors.
pub(crate) fn decode<T: DeserializeOwned>(
    response: Response<serde_json::Value>,
) -> Result<Response<T>, serde_json::Error> {
    let Response { data, errors } = response;
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
