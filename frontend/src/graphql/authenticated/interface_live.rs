//! Scope: The subscription for the live checks of the interface view; not the view (`interfaces`).
//!
//! The live checks of the interface view's rows (`AccessibleDevice.interfaceLive`,
//! `components::device::interfaces::live`), with the neighbor findings of
//! `super::neighbor_findings`.
use super::{
    interfaces::RowAction,
    neighbor_findings::{NeighborFinding, SeenNeighbor},
    schema::schema,
    types::{DeviceRef, DeviceTarget, InterfaceRef, PoeOutStatus, PoeProblem},
};

#[derive(cynic::QueryVariables, Debug)]
pub struct RefreshInterfaceLiveVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "RefreshInterfaceLiveVariables"
)]
pub struct RefreshInterfaceLive {
    #[arguments(id: $id)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub refresh_interface_live: bool,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct InterfaceLiveVariables {
    pub id: i32,
    pub target: DeviceTarget,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "InterfaceLiveVariables"
)]
pub struct InterfaceLiveSubscription {
    pub topology: LiveTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "InterfaceLiveVariables"
)]
pub struct LiveTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<LiveDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "DeviceAccess",
    variables = "InterfaceLiveVariables"
)]
pub struct LiveDevice {
    #[arguments(
        target: $target,
    )]
    pub access: Option<LiveAccess>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct LiveAccess {
    pub interface_live_age: i32,
    pub interface_live: InterfaceLive,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct InterfaceLive {
    pub rows: Vec<LiveRow>,
    pub other_findings: Vec<NeighborFinding>,
    pub other_addresses: Vec<DeviceOnlyAddress>,
    pub other_leases: Vec<DeviceOnlyLease>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LiveRow {
    pub interface: i32,
    pub seen: Vec<SeenNeighbor>,
    pub ospf: Vec<OspfNeighbor>,
    pub ospf_missing: Vec<DeviceRef>,
    pub ospf_untagged: bool,
    pub bridges: Vec<BridgeAnswer>,
    pub findings: Vec<NeighborFinding>,
    pub device_only_addresses: Vec<DeviceOnlyAddress>,
    pub device_only_leases: Vec<DeviceOnlyLease>,
    pub poe: Vec<PoeMismatch>,
    pub actions: Vec<RowAction>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct OspfNeighbor {
    pub instance: String,
    pub address: String,
    pub router_id: String,
    pub state: String,
    pub adjacent: bool,
    pub device: Option<DeviceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct BridgeAnswer {
    pub device: DeviceRef,
    pub address: Option<String>,
    pub answers: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoeMismatch {
    pub port: InterfaceRef,
    pub problem: PoeProblem,
    pub status: Option<PoeOutStatus>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceOnlyAddress {
    pub address: String,
    pub interface_name: String,
    pub interface: Option<i32>,
    pub disabled: bool,
    pub elsewhere: Option<DeviceRef>,
    pub unassigned: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceOnlyLease {
    pub address: String,
    pub mac: Option<String>,
    pub client_id: Option<String>,
    pub server: Option<String>,
    pub name: Option<String>,
    pub disabled: bool,
    pub settings: Vec<String>,
    pub elsewhere: Option<DeviceRef>,
    pub unassigned: bool,
}
