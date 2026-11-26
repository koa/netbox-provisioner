//! Scope: The query of the page „Zustand“; nothing else.
//!
//! The findings of every site's routers, most urgent first, and their health
//! (`networkCondition`, `docs/concepts/monitoring.md`).
use super::{
    network_map::{MapEdgeKind, MapNodeDetails},
    types::{DeviceRef, ErrorInfo, InterfaceRef, PoeProblem},
};
use crate::graphql::schema::schema;

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct Condition {
    pub topology: ConditionTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "TopologyHolder")]
pub struct ConditionTopology {
    pub network_condition: NetworkCondition,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct NetworkCondition {
    pub findings: Vec<ConditionFinding>,
    pub devices: Vec<MapNodeDetails>,
    pub lsdb_error: Option<ErrorInfo>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ConditionFinding {
    pub kind: ConditionKind,
    pub device: DeviceRef,
    pub link: Option<ConditionLink>,
    pub port: Option<InterfaceRef>,
    pub poe: Option<PoeProblem>,
    /// nanoseconds
    pub uptime: Option<u64>,
    pub uplink: Option<UplinkQuality>,
    pub client: Option<ConditionClient>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ConditionClient {
    pub mac_address: String,
    pub hostname: Option<String>,
    pub ssid: String,
    pub access_point: String,
    pub interface: String,
    pub signal: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct ConditionLink {
    pub kind: MapEdgeKind,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
pub struct UplinkQuality {
    pub gateway: String,
    pub loss: f64,
    pub rtt_ms: Option<f64>,
}

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionKind {
    Unreachable,
    LinkDown,
    #[cynic(rename = "LINK_IPV6_DOWN")]
    LinkIpv6Down,
    NoDefaultRoute,
    UplinkLoss,
    SignalBad,
    SignalWeak,
    Rebooted,
    PoeMismatch,
    PoeNoLoad,
    #[cynic(fallback)]
    Other,
}
