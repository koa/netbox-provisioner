//! Scope: The queries of the residents' endpoint; not the admins' (`authenticated`).
//!
//! The queries of the residents' endpoint (`/graphql/resident`), checked against its own schema
//! (`schema`), apart from the admins'.
pub mod change;
pub mod live;
pub mod residents;
pub mod schema;
