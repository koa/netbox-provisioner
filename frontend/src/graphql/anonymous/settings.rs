//! What the login needs (`authentication`, `pages`).
use super::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "anonymous", graphql_type = "QueryAnonymous")]
pub struct Settings {
    pub authentication: Authentication,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "anonymous", graphql_type = "AuthenticationData")]
pub struct Authentication {
    pub client_id: String,
    pub issuer_url: String,
    pub scopes: Vec<String>,
}
