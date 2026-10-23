use crate::error::FrontendError;
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use std::sync::LazyLock;
use yew::Component;
use yew::html::Scope;
use yew_oauth2::prelude::{Authentication, OAuth2Context};

pub mod anonymous;
pub mod authenticated;

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
    let mut headers = HeaderMap::new();
    if let Some((
        OAuth2Context::Authenticated(Authentication {
            access_token,
            id_token,
            ..
        }),
        _,
    )) = scope.context::<OAuth2Context>(Default::default())
    {
        // The ID token carries the user's name, Pocket-ID's access token only `sub`; a refresh
        // may come without a new ID token
        let token = id_token.unwrap_or(access_token);
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
    }
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    post_graphql::<Q>(&client, GRAPHQL_AUTHENTICATED_URL.as_str(), request).await
}

/// `graphql_client`'s `post_graphql`, but a response without success (a token the server
/// refuses: `401` or `403` with a text) is that text, not a failure to decode it as JSON.
async fn post_graphql<Q: GraphQLQuery>(
    client: &reqwest::Client,
    url: &str,
    variables: Q::Variables,
) -> Result<Response<Q::ResponseData>, FrontendError> {
    let response = client.post(url).json(&Q::build_query(variables)).send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(FrontendError::Server { status: status.as_u16(), text });
    }
    Ok(response.json().await?)
}

pub async fn query_anonymous<Q: GraphQLQuery>(
    request: Q::Variables,
) -> Result<Q::ResponseData, FrontendError> {
    let client = reqwest::Client::builder().build()?;
    let response = post_graphql::<Q>(&client, GRAPHQL_ANONYMOUS_URL.as_str(), request).await?;
    if let Some(data) = response.data {
        Ok(data)
    } else {
        Err(FrontendError::Graphql(response.errors.unwrap_or_default()))
    }
}
