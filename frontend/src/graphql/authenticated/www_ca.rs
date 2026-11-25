//! Scope: The query whether the root CA of WebFig over HTTPS can be downloaded; nothing else.
use crate::graphql::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct WwwCa {
    pub www_ca: bool,
}
