//! Scope: The queries of what a resident's view reads from the devices: the devices in a
//! network, the devices behind its ports, the ports the devices are plugged in, which devices
//! behind its elements are up; nothing else.
use super::residents::{Named, PortKind};
use crate::graphql::schema::schema;

#[derive(cynic::QueryVariables, Debug)]
pub struct NetworkDevicesVariables {
    pub resident: i32,
    pub vlan: i32,
}

/// The devices in one of the resident's networks: leases and WLAN clients.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "NetworkDevicesVariables")]
pub struct NetworkDevicesQuery {
    #[arguments(resident: $resident, vlan: $vlan)]
    pub network_devices: NetworkDevices,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NetworkDevices {
    pub devices: Vec<NetworkDevice>,
    pub unreachable: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NetworkDevice {
    pub mac_address: String,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub wlan: Option<WlanConnection>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct WlanConnection {
    pub ssid: String,
    pub access_point: String,
    pub signal: i32,
    pub band: Option<WlanBand>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct WlanBand {
    pub frequency: WlanFrequency,
    pub standard: Option<WlanStandard>,
}

#[derive(cynic::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlanFrequency {
    #[cynic(rename = "GHZ_2")]
    Ghz2,
    #[cynic(rename = "GHZ_5")]
    Ghz5,
    #[cynic(rename = "GHZ_6")]
    Ghz6,
    /// a band of a newer server
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlanStandard {
    A,
    B,
    G,
    N,
    Ac,
    Ax,
    Be,
    /// a standard of a newer server
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct SeenUplinksVariables {
    pub resident: i32,
}

/// Where the routers see the resident's devices plugged in, where it isn't NetBox's Anschluss.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "SeenUplinksVariables")]
pub struct SeenUplinksQuery {
    #[arguments(resident: $resident)]
    pub seen_uplinks: SeenUplinks,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct SeenUplinks {
    pub devices: Vec<SeenUplink>,
    pub unreachable: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct SeenUplink {
    pub device: i32,
    pub port: Named,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct PortDevicesVariables {
    pub resident: i32,
}

/// The devices behind the resident's ports in one of its networks.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "PortDevicesVariables")]
pub struct PortDevicesQuery {
    #[arguments(resident: $resident)]
    pub port_devices: PortDevices,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct PortDevices {
    pub ports: Vec<PortHosts>,
    pub unreachable: Vec<Named>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct PortHosts {
    pub kind: PortKind,
    pub port: i32,
    pub devices: Vec<NetworkDevice>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct LiveStateVariables {
    pub resident: i32,
}

/// Which devices behind the resident's elements are up, as the OSPF routers see them.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query", variables = "LiveStateVariables")]
pub struct LiveStateQuery {
    #[arguments(resident: $resident)]
    pub live_state: LiveState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct LiveState {
    pub known: bool,
    pub devices: Vec<DeviceLive>,
    pub ports: Vec<PortLive>,
    pub wlans: Vec<WlanLive>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct DeviceLive {
    pub device: Named,
    pub reached: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct PortLive {
    pub kind: PortKind,
    pub id: i32,
    pub switch: Named,
    pub reached: Option<bool>,
    pub link: Option<bool>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct WlanLive {
    pub wlan: i32,
    pub access_points: i32,
    pub unreached: Vec<Named>,
}
