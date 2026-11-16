//! Scope: The query whether the root CA of WebFig over HTTPS can be downloaded; nothing else.
use super::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct WwwCa {
    pub www_ca: bool,
}
