//! The jobs saved before a restart that didn't go on (`components::saved_jobs`).
use super::{
    schema::schema,
    types::{ApplyState, UpdateState},
};

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "QueryAuthenticated")]
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
    pub phase: Option<Phase>,
}

/// The state it was saved in: only an apply's or an update's go on after a restart.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(graphql_type = "JobPhaseObject")]
pub enum Phase {
    ApplyPhase(ApplyPhase),
    UpdatePhase(UpdatePhase),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ApplyPhase {
    #[cynic(alias, rename = "state")]
    pub apply: ApplyState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct UpdatePhase {
    #[cynic(alias, rename = "state")]
    pub update: UpdateState,
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
#[cynic(graphql_type = "MutationAuthenticated", variables = "DiscardSavedJobVariables")]
pub struct DiscardSavedJob {
    #[arguments(deviceId: $device_id)]
    #[expect(dead_code, reason = "selected because a mutation needs a field; the page reloads")]
    pub discard_saved_job: bool,
}
