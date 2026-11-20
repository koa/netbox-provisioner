//! Scope: The queries and mutations of a resident's change: its plan for the preview, saving it,
//! how saving went; not what a resident sees (`residents`).
use super::{residents::Named, schema::schema};
use crate::error::ServerProblem;

/// A change of a resident (`@oneOf`).
#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub enum ResidentChange {
    Wlan(WlanInput),
    PortVlan(PortVlanChange),
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct PortVlanChange {
    pub port: i32,
    pub kind: PortKindInput,
    /// none: no network there
    pub vlan: Option<i32>,
}

/// The kind of a port row, as input (the output's has a fallback).
#[derive(cynic::Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[cynic(schema = "resident", graphql_type = "PortKind")]
pub enum PortKindInput {
    Interface,
    Socket,
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct WlanInput {
    pub wlan: i32,
    pub ssid: Option<String>,
    pub psk: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ChangePlanVariables {
    pub resident: i32,
    pub change: ResidentChange,
}

/// What a change would write, for the preview.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "ChangePlanVariables"
)]
pub struct ChangePlanQuery {
    #[arguments(resident: $resident, change: $change)]
    pub change_plan: ResidentPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ResidentPlan {
    pub fingerprint: String,
    pub wlans: Vec<UpdateWirelessLan>,
    pub ports: Vec<UpdatePortVlan>,
    pub devices: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct UpdatePortVlan {
    pub vlan: Option<VlanRef>,
    pub vlan_before: Option<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct VlanRef {
    pub vid: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct UpdateWirelessLan {
    pub ssid: Option<String>,
    pub ssid_before: String,
    pub psk_set: bool,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SaveChangeVariables {
    pub resident: i32,
    pub change: ResidentChange,
    pub fingerprint: String,
}

/// Saves a change, if its plan is still the one previewed.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "MutationResident",
    variables = "SaveChangeVariables"
)]
pub struct SaveChange {
    #[arguments(resident: $resident, change: $change, fingerprint: $fingerprint)]
    pub save_change: SavedChange,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SavedChangeVariables {
    pub job: i32,
}

/// How saving a change stands.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "SavedChangeVariables"
)]
pub struct SavedChangeQuery {
    #[arguments(job: $job)]
    pub saved_change: Option<SavedChange>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct SavedChange {
    pub job: i32,
    pub state: SaveState,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
#[cynic(schema = "resident")]
pub enum SaveState {
    Saving,
    Saved,
    Failed,
    /// a state of a newer server
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ErrorInfo {
    pub code: String,
    pub params: Vec<ErrorParam>,
    pub message: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ErrorParam {
    pub name: String,
    pub value: String,
}

impl From<&ErrorInfo> for ServerProblem {
    fn from(problem: &ErrorInfo) -> Self {
        ServerProblem {
            code: problem.code.as_str().into(),
            params: problem
                .params
                .iter()
                .map(|param| (param.name.clone(), param.value.clone()))
                .collect(),
            message: problem.message.as_str().into(),
        }
    }
}
