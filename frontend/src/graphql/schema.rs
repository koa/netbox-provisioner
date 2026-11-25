//! Scope: The server's schema for cynic; nothing else.
//!
//! Registered by `build.rs` as the default schema: every query struct is checked against it
//! when the frontend compiles. The types several queries share are in
//! [`super::authenticated::types`].
#[cynic::schema("provisioner")]
pub mod schema {}
