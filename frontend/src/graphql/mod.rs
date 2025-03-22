use crate::error::FrontendError;
use graphql_client::GraphQLQuery;
use graphql_client::reqwest::post_graphql;
use lazy_static::lazy_static;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use yew::html::Scope;
use yew::{Callback, Component};
use crate::data::UserSessionData;

pub mod anonymous;

lazy_static! {
    static ref GRAPHQL_AUTHENTICATED_URL: String = format!("{}/graphql", host());
    static ref GRAPHQL_ANONYMOUS_URL: String = format!("{}/graphql_anonymous", host());
}

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
    let mut headers = HeaderMap::new();
    if let Some((session_data, _)) = scope.context::<UserSessionData>(Default::default()) {
        if let Some(access_token) = session_data.jwt() {
            headers.insert(AUTHORIZATION, format!("Bearer {access_token}").parse()?);
        }
    }
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    let response =
        post_graphql::<Q, _>(&client, GRAPHQL_AUTHENTICATED_URL.as_str(), request).await?;
    if let Some(data) = response.data {
        Ok(data)
    } else {
        Err(FrontendError::Graphql(response.errors.unwrap_or_default()))
    }
}
pub async fn query_anonymous<Q: GraphQLQuery>(
    request: Q::Variables,
) -> Result<Q::ResponseData, FrontendError> {
    let client = reqwest::Client::builder().build()?;
    let response = post_graphql::<Q, _>(&client, GRAPHQL_ANONYMOUS_URL.as_str(), request).await?;
    if let Some(data) = response.data {
        Ok(data)
    } else {
        Err(FrontendError::Graphql(response.errors.unwrap_or_default()))
    }
}
