//! The network map (`components::network_map`): the devices and links as the provisioner reads
//! them (`networkState`), the slower details beside (`networkStateDetails`), and the layout
//! published to NetBox (`publishLayout`).
use super::{
    schema::schema,
    types::{DeviceRef, InterfaceRef, PoeProblem, WideInt},
};

#[derive(cynic::QueryVariables, Debug)]
pub struct MapVariables {
    pub site: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "MapVariables"
)]
pub struct NetworkMap {
    pub allow_netbox_write: bool,
    pub topology: MapTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "MapVariables"
)]
pub struct MapTopology {
    #[arguments(siteId: $site)]
    pub network_state: NetworkState,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetworkState {
    pub read_from: Option<DeviceRef>,
    pub age: i32,
    pub error: Option<String>,
    pub outside: i32,
    pub outside_at: Vec<i32>,
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapNode {
    pub device: DeviceRef,
    pub routeros: bool,
    pub state: NodeState,
    pub site_name: Option<String>,
    pub position: Option<MapPosition>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapEdge {
    pub kind: MapEdgeKind,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    pub state: MapEdgeState,
    pub reason: Option<MapDownReason>,
    pub reason_device: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "MapVariables"
)]
pub struct NetworkMapDetails {
    pub topology: DetailsTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "MapVariables"
)]
pub struct DetailsTopology {
    #[arguments(siteId: $site)]
    pub network_state_details: NetworkStateDetails,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NetworkStateDetails {
    pub nodes: Vec<MapNodeDetails>,
    pub poe_edges: Vec<MapPoeEdge>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapNodeDetails {
    pub device: DeviceRef,
    pub health: Vec<MapHealth>,
    pub resources: Option<MapResources>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapHealth {
    pub name: String,
    pub value: String,
    pub unit: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapResources {
    pub uptime: String,
    pub cpu_load: i32,
    pub free_memory: WideInt,
    pub total_memory: WideInt,
    pub free_hdd_space: WideInt,
    pub version: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MapPoeEdge {
    pub pse: InterfaceRef,
    pub pd: InterfaceRef,
    pub status: Option<String>,
    pub power: Option<f64>,
    pub problem: Option<PoeProblem>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct PublishLayoutVariables {
    pub positions: Vec<DevicePositionInput>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "MutationAuthenticated",
    variables = "PublishLayoutVariables"
)]
pub struct PublishLayout {
    #[arguments(positions: $positions)]
    pub publish_layout: i32,
}

#[derive(cynic::InputObject, Debug, Clone)]
#[cynic(schema = "authenticated")]
pub struct DevicePositionInput {
    pub device_id: i32,
    pub x: i32,
    pub y: i32,
}

/// How a device was reached (`NodeState`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum NodeState {
    Reached,
    Isolated,
    Missing,
    NotOspf,
    #[cynic(fallback)]
    Other,
}

/// What a link is (`MapEdgeKind`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum MapEdgeKind {
    Cable,
    Radio,
    #[cynic(fallback)]
    Other,
}

/// Whether OSPF runs over a link (`MapEdgeState`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum MapEdgeState {
    Up,
    V3Down,
    Down,
    NotOspf,
    #[cynic(fallback)]
    Other,
}

/// Why a link is down (`MapDownReason`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum MapDownReason {
    RouterMissing,
    NoSegment,
    NotOnSegment,
    #[cynic(fallback)]
    Other,
}
