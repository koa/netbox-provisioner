//! The input naming a helper and its start (`Helper`, `@oneOf`): what its plan, its preview and
//! the findings after it are computed for. `components::netbox_helper::choice::helper_input`
//! builds it from the card's choice.
use super::schema::schema;

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub enum Helper {
    Loopback(i32),
    Connection(ConnectionInput),
    Disconnect(DisconnectInput),
    Flags(FlagsInput),
    CustomerNet(CustomerNetInput),
    Poe(PoeInput),
    PoeRemove(PoeRemoveInput),
    RadioBridge(RadioBridgeInput),
    RadioDisconnect(RadioDisconnectInput),
    CustomerDisconnect(CustomerDisconnectInput),
    PortVlan(PortVlanInput),
    CableTakeover(CableTakeoverInput),
    BridgeLeave(BridgeLeaveInput),
    NewDevice(NewDeviceInput),
    AddressTakeover(AddressTakeoverInput),
    LeaseTakeover(LeaseTakeoverInput),
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct ConnectionInput {
    pub interface: i32,
    pub peer: i32,
    pub mtu: Option<i32>,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct DisconnectInput {
    pub device: i32,
    pub interface: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct FlagsInput {
    pub device: i32,
    pub changes: Vec<TagChange>,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct TagChange {
    pub interface: i32,
    pub tag: InterfaceTag,
    pub set: bool,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct CustomerNetInput {
    pub device: i32,
    pub target: CustomerNetTarget,
    pub parent: Option<i32>,
    pub length: Option<i32>,
    pub parent_v6: Option<i32>,
    pub dhcp: bool,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub enum CustomerNetTarget {
    Interface(i32),
    Vlan(i32),
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct PoeInput {
    pub device: i32,
    pub pse: i32,
    pub pd: Option<i32>,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct PoeRemoveInput {
    pub device: i32,
    pub port: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct RadioBridgeInput {
    pub ap: i32,
    pub station: i32,
    pub ap_router_port: Option<i32>,
    pub station_router_port: Option<i32>,
    pub ap_address: Option<String>,
    pub station_address: Option<String>,
    pub ssid: Option<String>,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct RadioDisconnectInput {
    pub device: i32,
    pub radio: i32,
    pub wireless_link: bool,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct CustomerDisconnectInput {
    pub device: i32,
    pub interface: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct PortVlanInput {
    pub device: i32,
    pub port: i32,
    pub vlan: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct CableTakeoverInput {
    pub port: i32,
    pub peer: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct BridgeLeaveInput {
    pub device: i32,
    pub port: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct NewDeviceInput {
    pub port: i32,
    pub name: String,
    pub device_type: i32,
    pub role: i32,
    pub far_port: String,
    pub wlan_group: Option<i32>,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct AddressTakeoverInput {
    pub device: i32,
    pub interface: i32,
    pub address: String,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct LeaseTakeoverInput {
    pub device: i32,
    pub address: String,
    pub mac: String,
    pub name: String,
    pub device_type: i32,
    pub role: i32,
}

/// A tag the provisioner acts on (`InterfaceTag`); one a newer backend has keeps its name.
#[derive(cynic::Enum, Clone, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum InterfaceTag {
    Ospf,
    Dhcp,
    DhcpClient,
    Nat,
    Internet,
    #[cynic(fallback)]
    Other(String),
}
