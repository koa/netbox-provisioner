//! Scope: The authenticated endpoint's schema for cynic; nothing else.
//!
//! The authenticated endpoint's schema for cynic, registered by `build.rs` as "authenticated"
//! from `schema.graphql`: every query struct of [`super`] names it (`schema = "authenticated"`)
//! and is checked against it when the frontend compiles; the anonymous endpoint has its own.
//! The types several queries share are in [`super::types`].
#[cynic::schema("authenticated")]
pub mod schema {}
