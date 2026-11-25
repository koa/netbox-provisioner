//! Scope: The queries for the jobs saved before a restart; nothing else.
//!
//! The jobs saved before a restart that didn't go on (`components::saved_jobs`).
use super::types::JobPhase;
use crate::graphql::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct SavedJobs {
    pub allow_apply: bool,
    pub saved_jobs: Vec<Waiting>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct Waiting {
    pub device_id: i32,
    pub device_name: Option<String>,
    pub kind: Option<SavedJobKind>,
    pub age: Option<u64>,
    pub unreadable: Option<String>,
    pub held_back: Option<HeldBack>,
    pub phase: Option<JobPhase>,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedJobKind {
    Apply,
    Update,
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldBack {
    DeviceGone,
    BatchIncomplete,
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct DiscardSavedJobVariables {
    pub device_id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Mutation", variables = "DiscardSavedJobVariables")]
pub struct DiscardSavedJob {
    #[arguments(deviceId: $device_id)]
    #[expect(
        dead_code,
        reason = "selected because a mutation needs a field; the page reloads"
    )]
    pub discard_saved_job: bool,
}
