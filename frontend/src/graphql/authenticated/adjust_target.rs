//! The credentials the backend knows by name (`listCredentials`), for
//! `components::device::adjust_target`.
use super::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct ListCredentials {
    pub list_credentials: Vec<String>,
}
