//! Scope: The query of a resident's saved changes and how far each has reached its devices;
//! not saving them (`change`).
use super::{change::PortKindInput, schema::schema};

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
    pub undoable: bool,
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
    Outward,
    #[cynic(fallback)]
    Other(String),
}

#[derive(cynic::QueryVariables, Debug)]
pub struct UndoChangeVariables {
    pub resident: i32,
    pub change: i32,
}

/// What takes a saved change back, as the parts of a new change.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "UndoChangeVariables"
)]
pub struct UndoChangeQuery {
    #[arguments(resident: $resident, change: $change)]
    pub undo_change: Vec<UndoStep>,
}

/// One this frontend doesn't know yet is `Unknown`.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub enum UndoStep {
    WlanUndo(WlanUndo),
    PortVlanUndo(PortVlanUndo),
    ConnectDeviceUndo(ConnectDeviceUndo),
    DisconnectDeviceUndo(DisconnectDeviceUndo),
    DeviceRoomUndo(DeviceRoomUndo),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct WlanUndo {
    pub wlan: i32,
    pub ssid: Option<String>,
    pub psk: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct PortVlanUndo {
    pub port: i32,
    pub kind: PortKindInput,
    pub vlan: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct ConnectDeviceUndo {
    pub device: i32,
    pub port: i32,
    pub kind: PortKindInput,
    pub device_port: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct DisconnectDeviceUndo {
    pub device: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct DeviceRoomUndo {
    pub device: i32,
    pub location: i32,
}
