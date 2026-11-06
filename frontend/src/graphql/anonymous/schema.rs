//! The anonymous endpoint's schema for cynic, registered by `build.rs` as "anonymous": every
//! query struct of [`super`] names it (`schema = "anonymous"`).
#[cynic::schema("anonymous")]
pub mod schema {}
