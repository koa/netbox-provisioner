//! Scope: The queries and mutations of a resident's changes: their plan for the preview, saving
//! them, how saving went; not what a resident sees (`residents`).
use super::residents::Named;
use crate::error::ServerProblem;
use crate::graphql::schema::schema;

/// A change of a resident (`@oneOf`).
#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
pub enum ResidentChange {
    Wlan(WlanInput),
    PortVlan(PortVlanChange),
    ConnectDevice(ConnectDeviceChange),
    DisconnectDevice(i32),
    DeviceRoom(DeviceRoomChange),
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
pub struct DeviceRoomChange {
    pub device: i32,
    pub location: i32,
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
pub struct ConnectDeviceChange {
    pub device: i32,
    pub port: i32,
    pub kind: PortKindInput,
    /// none: its Anschluss
    pub device_port: Option<i32>,
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
pub struct PortVlanChange {
    pub port: i32,
    pub kind: PortKindInput,
    /// none: no network there
    pub vlan: Option<i32>,
}

/// The kind of a port row, as input (the output's has a fallback).
#[derive(cynic::Enum, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cynic(graphql_type = "PortKind")]
pub enum PortKindInput {
    Interface,
    Socket,
}

#[derive(cynic::InputObject, Debug, Clone, PartialEq)]
pub struct WlanInput {
    pub wlan: i32,
    pub ssid: Option<String>,
    pub psk: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct ChangePlanVariables {
    pub resident: i32,
    pub changes: Vec<ResidentChange>,
}

/// What changes would write together, for the preview.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "ChangePlanVariables")]
pub struct ChangePlanQuery {
    #[arguments(resident: $resident, changes: $changes)]
    pub change_plan: ResidentPlan,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ResidentPlan {
    pub fingerprint: String,
    pub wlans: Vec<UpdateWirelessLan>,
    pub ports: Vec<UpdatePortVlan>,
    pub cables_added: Vec<CreateCable>,
    pub cables_removed: Vec<DeleteCable>,
    pub rooms: Vec<UpdateDeviceLocation>,
    pub cut_off: Vec<Named>,
    pub devices: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct CreateCable {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct DeleteCable {
    pub port: InterfaceRef,
    pub far: Option<InterfaceRef>,
}

/// A port and its device, by name.
#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(graphql_type = "PlanInterface")]
pub struct InterfaceRef {
    pub name: String,
    pub device: DeviceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(graphql_type = "PlanDevice")]
pub struct DeviceRef {
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct UpdateDeviceLocation {
    pub device: DeviceRef,
    pub location: Option<LocationRef>,
    pub location_before: Option<LocationRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct LocationRef {
    pub path: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct UpdatePortVlan {
    pub vlan: Option<VlanRef>,
    pub vlan_before: Option<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct VlanRef {
    pub vid: i32,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct UpdateWirelessLan {
    pub ssid: Option<String>,
    pub ssid_before: String,
    pub psk_set: bool,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SaveChangeVariables {
    pub resident: i32,
    pub changes: Vec<ResidentChange>,
    pub fingerprint: String,
}

/// Saves changes together, if their plan is still the one previewed.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Mutation", variables = "SaveChangeVariables")]
pub struct SaveChange {
    #[arguments(resident: $resident, changes: $changes, fingerprint: $fingerprint)]
    pub save_change: SavedChange,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SavedChangeVariables {
    pub job: i32,
}

/// How saving a change stands.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "SavedChangeVariables")]
pub struct SavedChangeQuery {
    #[arguments(job: $job)]
    pub saved_change: Option<SavedChange>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct SavedChange {
    pub job: i32,
    pub state: SaveState,
    pub error: Option<ErrorInfo>,
}

#[derive(cynic::Enum, Debug, Clone, PartialEq, Eq)]
pub enum SaveState {
    Saving,
    Saved,
    Failed,
    /// a state of a newer server
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ErrorInfo {
    pub code: String,
    pub params: Vec<ErrorParam>,
    pub message: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
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
