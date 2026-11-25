//! Scope: The query for the credentials the backend knows by name; nothing else.
//!
//! The credentials the backend knows by name (`listCredentials`), for
//! `components::device::adjust_target`.
use crate::graphql::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct ListCredentials {
    pub list_credentials: Vec<String>,
}
