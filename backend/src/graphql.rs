use crate::config::CONFIG;
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};

pub type AuthenticatedGraphqlSchema = Schema<QueryAuthenticated, EmptyMutation, EmptySubscription>;
pub type AnonymousGraphqlSchema = Schema<QueryAnonymous, EmptyMutation, EmptySubscription>;

pub struct QueryAuthenticated;
pub struct QueryAnonymous;

pub fn create_schema() -> AuthenticatedGraphqlSchema {
    Schema::build(QueryAuthenticated, EmptyMutation, EmptySubscription).finish()
}
pub fn create_anonymous_schema() -> AnonymousGraphqlSchema {
    Schema::build(QueryAnonymous, EmptyMutation, EmptySubscription).finish()
}

#[Object]
impl QueryAuthenticated {
    async fn dummy(&self) -> String {
        String::from("dummy value")
    }
}
#[Object]
impl QueryAnonymous {
    /// gives the coordinates for authentication
    async fn authentication(&self) -> AuthenticationData {
        AuthenticationData {
            client_id: CONFIG.auth_client_id(),
            auth_url: CONFIG.auth_url(),
            token_url: CONFIG.auth_token_url(),
        }
    }
}
#[derive(SimpleObject)]
struct AuthenticationData {
    client_id: &'static str,
    token_url: String,
    auth_url: String,
}
