//! Scope: The query of a resident's saved changes and how far each has reached its devices;
//! not saving them (`change`).
use super::schema::schema;

#[derive(cynic::QueryVariables, Debug)]
pub struct ChangesVariables {
    pub resident: i32,
}

/// The resident's saved changes, the newest first.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "ChangesVariables"
)]
pub struct ChangesQuery {
    #[arguments(resident: $resident)]
    pub changes: Vec<ChangeState>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ChangeState {
    pub id: i32,
    pub by: String,
    pub age: i32,
    pub devices: Vec<ChangeDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ChangeDevice {
    pub device: RoundDevice,
    pub state: RolloutState,
    pub reasons: Vec<HoldReason>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct RoundDevice {
    pub name: String,
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
#[cynic(schema = "resident")]
pub enum RolloutState {
    Pending,
    RolledOut,
    Waiting,
    Deferred,
    Failed,
    Manual,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
#[cynic(schema = "resident")]
pub enum HoldReason {
    PoeCut,
    CapOff,
    ApiClientsOutside,
    RadioLink,
    Rewire,
    #[cynic(fallback)]
    Other(String),
}
