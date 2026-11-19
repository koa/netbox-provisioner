//! Scope: The queries and mutations of a resident's change: its plan for the preview, saving it,
//! how saving went; not what a resident sees (`residents`).
use super::{residents::Named, schema::schema};
use crate::error::ServerProblem;

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct WlanInput {
    pub wlan: i32,
    pub ssid: Option<String>,
    pub psk: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct WlanPlanVariables {
    pub resident: i32,
    pub change: WlanInput,
}

/// What changing a WLAN would write, for the preview.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "WlanPlanVariables"
)]
pub struct WlanPlanQuery {
    #[arguments(resident: $resident, change: $change)]
    pub wlan_plan: ResidentPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ResidentPlan {
    pub fingerprint: String,
    pub wlans: Vec<UpdateWirelessLan>,
    pub devices: Vec<Named>,
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
pub struct SaveWlanVariables {
    pub resident: i32,
    pub change: WlanInput,
    pub fingerprint: String,
}

/// Saves the WLAN's change, if its plan is still the one previewed.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "MutationResident",
    variables = "SaveWlanVariables"
)]
pub struct SaveWlan {
    #[arguments(resident: $resident, change: $change, fingerprint: $fingerprint)]
    pub save_wlan: SavedChange,
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
