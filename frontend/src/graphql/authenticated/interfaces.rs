//! A device's interfaces sorted by task (`DeviceAccess.interfaceView`,
//! `components::device::interfaces`), the findings of `super::findings` grouped onto the rows.
use super::{
    findings::Finding,
    schema::schema,
    types::{AddressRef, DeviceRef, InterfaceRef, VlanRef},
};

#[derive(cynic::QueryVariables, Debug)]
pub struct DeviceInterfacesVariables {
    pub device: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "DeviceInterfacesVariables"
)]
pub struct DeviceInterfaces {
    pub topology: InterfacesTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceInterfacesVariables"
)]
pub struct InterfacesTopology {
    pub netbox_url: String,
    #[arguments(id: $device)]
    pub device_by_id: Option<InterfacesDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct InterfacesDevice {
    pub interface_view: InterfaceView,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct InterfaceView {
    pub rows: Vec<InterfaceRow>,
    pub device_findings: Vec<Finding>,
    pub other_findings: Vec<Finding>,
    pub loopback_actions: Vec<RowAction>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct InterfaceRow {
    pub task: InterfaceTask,
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub ports: Vec<InterfaceRef>,
    pub port_rows: Vec<PortRow>,
    pub addresses: Vec<AddressRef>,
    pub ospf: bool,
    pub dhcp: bool,
    pub dhcp_client: bool,
    pub nat: bool,
    pub internet: bool,
    pub mtu: Option<i32>,
    pub untagged_vlan: Option<VlanRef>,
    pub tagged_vlans: Vec<VlanRef>,
    pub far_ends: Vec<InterfaceRef>,
    pub radio_path: Option<RadioPath>,
    pub gateways: Vec<DeviceRef>,
    pub free: bool,
    pub findings: Vec<Finding>,
    pub actions: Vec<RowAction>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PortRow {
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub far_ends: Vec<InterfaceRef>,
    pub actions: Vec<RowAction>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RadioPath {
    pub router_port: Option<InterfaceRef>,
    pub bridge_port: Option<InterfaceRef>,
    pub radio: Option<InterfaceRef>,
    pub ssid: Option<String>,
    pub peer_radio: Option<InterfaceRef>,
    pub peer_bridge_port: Option<InterfaceRef>,
    pub peer_router_port: Option<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RowAction {
    pub kind: ActionKind,
    pub helper: ActionHelper,
    pub interface: Option<i32>,
    pub peer: Option<InterfaceRef>,
    pub vlan: Option<i32>,
    pub seen: Option<SeenDevice>,
    pub address: Option<String>,
    pub lease: Option<LeaseSeen>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct SeenDevice {
    pub identity: String,
    pub board: String,
    pub interface_name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LeaseSeen {
    pub mac: String,
    pub name: Option<String>,
}

/// What a row is for (`InterfaceTask`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum InterfaceTask {
    Loopback,
    Uplink,
    Direct,
    RadioLink,
    Customer,
    Unknown,
    #[cynic(fallback)]
    Other,
}

/// How an action is offered (`ActionKind`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum ActionKind {
    Suggestion,
    Remove,
    AssignRole,
    Edit,
    #[cynic(fallback)]
    Other,
}

/// The helper an action opens (`ActionHelper`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum ActionHelper {
    Loopback,
    Connection,
    Disconnect,
    CustomerNet,
    RadioBridge,
    RadioDisconnect,
    CustomerDisconnect,
    PortVlan,
    CableTakeover,
    Poe,
    PoeRemove,
    Flags,
    BridgeLeave,
    NewDevice,
    AddressTakeover,
    LeaseTakeover,
    #[cynic(fallback)]
    Other,
}
