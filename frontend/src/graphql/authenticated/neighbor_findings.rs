//! Scope: The query for what the devices see at their ports against NetBox's cables; nothing else.
//!
//! What the devices see at their ports compared with NetBox's cables (`neighborFindings`,
//! `components::site::neighbor_check`); the interface view's live checks group the same type onto
//! their rows (`super::interface_live`).
use super::types::{DeviceRef, InterfaceRef, PortRef, Severity};
use crate::graphql::schema::schema;

#[derive(cynic::QueryVariables, Debug)]
pub struct RefreshNeighborsVariables {
    pub ids: Vec<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Mutation", variables = "RefreshNeighborsVariables")]
pub struct RefreshNeighbors {
    #[arguments(ids: $ids)]
    #[expect(dead_code, reason = "selected because a mutation needs a field")]
    pub refresh_neighbors: bool,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct NeighborFindingsVariables {
    pub id: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Subscription", variables = "NeighborFindingsVariables")]
pub struct NeighborFindings {
    pub topology: NeighborsTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    graphql_type = "TopologyHolder",
    variables = "NeighborFindingsVariables"
)]
pub struct NeighborsTopology {
    pub netbox_url: String,
    #[arguments(id: $id)]
    pub device_by_id: Option<NeighborsDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "DeviceAccess")]
pub struct NeighborsDevice {
    pub access: Option<NeighborsAccess>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "AccessibleDevice")]
pub struct NeighborsAccess {
    pub neighbors_age: i32,
    pub neighbor_findings: Vec<NeighborFinding>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NeighborFinding {
    pub severity: Severity,
    pub kind: NeighborFindingKind,
}

/// One this frontend doesn't know yet is `Unknown`.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
pub enum NeighborFindingKind {
    CableMissing(CableMissing),
    CableElsewhere(CableElsewhere),
    OtherFarPort(OtherFarPort),
    NeighborNotSeen(NeighborNotSeen),
    NeighborsNotInNetbox(NeighborsNotInNetbox),
    DiscoveryOff(DiscoveryOff),
    #[cynic(fallback)]
    Unknown,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct CableMissing {
    pub port: PortRef,
    pub seen: Vec<SeenNeighbor>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct CableElsewhere {
    pub port: PortRef,
    pub far_end: InterfaceRef,
    pub seen: Vec<SeenNeighbor>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct OtherFarPort {
    pub port: PortRef,
    pub far_end: InterfaceRef,
    pub seen: SeenNeighbor,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NeighborNotSeen {
    pub port: PortRef,
    pub far_end: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NeighborsNotInNetbox {
    pub port: PortRef,
    pub seen: Vec<SeenNeighbor>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct DiscoveryOff {
    pub list: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct SeenNeighbor {
    pub identity: String,
    pub interface_name: String,
    pub board: String,
    pub device: Option<DeviceRef>,
    pub interface: Option<InterfaceRef>,
}
