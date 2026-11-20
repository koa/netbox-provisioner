//! Scope: The queries of what a resident's view reads from the devices: the devices in a
//! network, the ports the devices are plugged in; nothing else.
use super::{residents::Named, schema::schema};

#[derive(cynic::QueryVariables, Debug)]
pub struct NetworkDevicesVariables {
    pub resident: i32,
    pub vlan: i32,
}

/// The devices in one of the resident's networks: leases and WLAN clients.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "NetworkDevicesVariables"
)]
pub struct NetworkDevicesQuery {
    #[arguments(resident: $resident, vlan: $vlan)]
    pub network_devices: NetworkDevices,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct NetworkDevices {
    pub devices: Vec<NetworkDevice>,
    pub unreachable: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct NetworkDevice {
    pub mac_address: String,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub wlan: Option<WlanConnection>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct WlanConnection {
    pub ssid: String,
    pub access_point: String,
    pub signal: i32,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SeenUplinksVariables {
    pub resident: i32,
}

/// Where the routers see the resident's devices plugged in, where it isn't NetBox's Anschluss.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "resident",
    graphql_type = "QueryResident",
    variables = "SeenUplinksVariables"
)]
pub struct SeenUplinksQuery {
    #[arguments(resident: $resident)]
    pub seen_uplinks: SeenUplinks,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct SeenUplinks {
    pub devices: Vec<SeenUplink>,
    pub unreachable: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "resident")]
pub struct SeenUplink {
    pub device: i32,
    pub port: Named,
}
