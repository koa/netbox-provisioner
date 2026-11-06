//! The backend's schema for cynic, registered by `build.rs` from `authenticated/schema.graphql`:
//! every query struct is checked against it when the frontend compiles. Each module of
//! [`super`] with cynic structs takes it in as `schema`; the types several queries share are
//! in [`super::types`].
#[cynic::schema("provisioner")]
pub mod schema {}
