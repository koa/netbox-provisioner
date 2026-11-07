//! Scope: The queries of the anonymous endpoint; not those after the login (`authenticated`).
//!
//! The queries of the anonymous endpoint (`/graphql_anonymous`), before the login, checked
//! against its own schema (`schema`), apart from the authenticated one.
pub mod schema;
pub mod settings;
