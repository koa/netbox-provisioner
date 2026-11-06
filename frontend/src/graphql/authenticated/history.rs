//! The device's journal entries for „Verlauf“ (`components::history`).
use super::{schema::schema, types::JobKind};

#[derive(cynic::QueryVariables, Debug)]
pub struct DeviceJournalVariables {
    pub device: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceJournalVariables",
)]
pub struct DeviceJournal {
    pub topology: Topology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceJournalVariables",
)]
pub struct Topology {
    #[arguments(id: $device)]
    pub device_by_id: Option<Device>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct Device {
    pub journal: Vec<JournalEntry>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct JournalEntry {
    pub id: i32,
    pub created: String,
    pub kind: Option<JobKind>,
    pub observation: Option<JournalObservation>,
    pub outcome: JournalOutcome,
    pub state: Option<String>,
    pub left: Vec<JournalLeft>,
    pub by: Option<String>,
    pub url: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct JournalObservation {
    pub found: Vec<JournalLeft>,
    pub fired: Vec<JournalLeft>,
    pub removed: Vec<JournalLeft>,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum JournalOutcome {
    Running,
    Done,
    Incomplete,
    Failed,
    Cancelled,
    Interrupted,
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum JournalLeft {
    ApplyFallback,
    UpdateFallback,
    WatchdogSafetyNet,
    #[cynic(fallback)]
    Other,
}
