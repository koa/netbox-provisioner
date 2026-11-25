//! Scope: The query for what the login needs; nothing else.
//!
//! What the login needs (`authentication`, `pages`).
use crate::graphql::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct Settings {
    pub authentication: Authentication,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "AuthenticationData")]
pub struct Authentication {
    pub client_id: String,
    pub issuer_url: String,
    pub scopes: Vec<String>,
}
