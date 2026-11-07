//! Scope: The queries for what the helpers offer to choose from; not a helper's plan
//! (`netbox_plan`).
//!
//! What the NetBox helpers offer to choose from (`components::netbox_helper`, its radio bridge
//! also in `components::helper_flow`): ports, PoE ports, radio bridges, prefixes, VLANs, device
//! types and roles.
use super::{
    helper::InterfaceTag,
    schema::schema,
    types::{
        DeviceRef, DeviceTarget, DeviceVariablesFields, InterfaceRef, PoeMode, PortRef, RadioRole,
        VlanRef,
    },
};

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct LinkPorts {
    pub topology: LinkPortsTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct LinkPortsTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<LinkPortsOwn>,
    #[arguments(filter: { hasRouteros: true })]
    pub all_devices: Vec<LinkPortsDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct LinkPortsOwn {
    pub name: String,
    pub link_ports: Vec<LinkPort>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct LinkPortsDevice {
    pub id: i32,
    pub name: String,
    pub site: Option<SiteName>,
    pub link_ports: Vec<LinkPort>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct SiteName {
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LinkPort {
    pub id: i32,
    pub name: String,
    pub label: Option<String>,
    pub mtu: Option<i32>,
    pub ospf: bool,
    pub ipv4: Option<String>,
    pub bridge: Option<String>,
    pub cabled: bool,
    pub connected: Vec<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct FlagInterfaces {
    pub topology: FlagInterfacesTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct FlagInterfacesTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<FlagInterfacesDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct FlagInterfacesDevice {
    pub flag_interfaces: Vec<FlagInterface>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct FlagInterface {
    pub id: i32,
    pub name: String,
    pub label: Option<String>,
    pub addresses: Vec<String>,
    pub bridge: Option<String>,
    pub tags: Vec<InterfaceTag>,
    pub connected: Vec<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct PoeOptionsQuery {
    pub topology: PoeOptionsTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct PoeOptionsTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PoeOptionsDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct PoeOptionsDevice {
    pub poe_options: PoeOptions,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq, Default)]
#[cynic(schema = "authenticated")]
pub struct PoeOptions {
    pub ports: Vec<PoePort>,
    pub peers: Vec<PoePeer>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoePeer {
    pub id: i32,
    pub name: String,
    pub ports: Vec<PoePort>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoePort {
    pub id: i32,
    pub name: String,
    pub label: Option<String>,
    pub poe: Option<PoeMode>,
    pub cabled: bool,
    pub connected: Vec<InterfaceRef>,
}

#[derive(cynic::QueryVariables, Debug)]
pub struct PoeObservedVariables {
    pub id: i32,
    pub target: DeviceTarget,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "PoeObservedVariables"
)]
pub struct PoeObservedQuery {
    pub topology: PoeObservedTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "PoeObservedVariables"
)]
pub struct PoeObservedTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PoeObservedDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "DeviceAccess",
    variables = "PoeObservedVariables"
)]
pub struct PoeObservedDevice {
    #[arguments(
        target: $target,
    )]
    pub access: Option<PoeObservedAccess>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "AccessibleDevice")]
pub struct PoeObservedAccess {
    pub poe_observed: Vec<PoeObserved>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoeObserved {
    pub port: PortRef,
    pub far: Option<InterfaceRef>,
    pub from_neighbor: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct RadioBridgeCandidates {
    pub topology: RadioBridgeTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct RadioBridgeTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<RadioBridgeDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct RadioBridgeDevice {
    pub radio_bridge_candidates: Vec<RadioBridgeCandidate>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RadioBridgeCandidate {
    pub device: DeviceRef,
    pub routeros: bool,
    pub radio: InterfaceRef,
    pub role: Option<RadioRole>,
    pub peer: Option<DeviceRef>,
    pub router_port: Option<InterfaceRef>,
    pub address: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct CustomerNetOptionsQuery {
    pub topology: CustomerNetTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct CustomerNetTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<CustomerNetDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct CustomerNetDevice {
    pub customer_net_options: CustomerNetOptions,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq, Default)]
#[cynic(schema = "authenticated")]
pub struct CustomerNetOptions {
    pub interfaces: Vec<CustomerNetInterface>,
    pub vlans: Vec<CustomerNetVlan>,
    pub parents: Vec<CustomerNetParent>,
    pub parents_v6: Vec<CustomerNetParent>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CustomerNetInterface {
    pub id: i32,
    pub name: String,
    pub label: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CustomerNetVlan {
    pub id: i32,
    pub name: String,
    pub vid: i32,
    pub interface: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct CustomerNetParent {
    pub id: i32,
    pub prefix: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "QueryAuthenticated",
    variables = "DeviceVariables"
)]
pub struct PortVlanOptionsQuery {
    pub topology: PortVlanTopology,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "DeviceVariables"
)]
pub struct PortVlanTopology {
    #[arguments(id: $id)]
    pub device_by_id: Option<PortVlanDevice>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct PortVlanDevice {
    pub port_vlan_options: PortVlanOptions,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq, Default)]
#[cynic(schema = "authenticated")]
pub struct PortVlanOptions {
    pub ports: Vec<PortVlanPort>,
    pub groups: Vec<PortVlanGroup>,
    pub group: Option<i32>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PortVlanPort {
    pub id: i32,
    pub name: String,
    pub label: Option<String>,
    pub vlan: Option<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PortVlanGroup {
    pub id: i32,
    pub name: String,
    pub vxlan: bool,
    pub vlans: Vec<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct NewDeviceOptionsQuery {
    pub new_device_options: NewDeviceOptions,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NewDeviceOptions {
    pub device_types: Vec<DeviceTypeOption>,
    pub roles: Vec<RoleOption>,
    pub wlan_groups: Vec<WlanGroupOption>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DeviceTypeOption {
    pub id: i32,
    pub model: String,
    pub part_number: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RoleOption {
    pub id: i32,
    pub name: String,
    pub slug: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WlanGroupOption {
    pub id: i32,
    pub name: String,
    pub controller: String,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct LeaseTakeoverOptionsQuery {
    pub lease_takeover_options: LeaseTakeoverOptions,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LeaseTakeoverOptions {
    pub device_types: Vec<ClientTypeOption>,
    pub roles: Vec<RoleOption>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ClientTypeOption {
    pub id: i32,
    pub model: String,
    pub manufacturer: String,
}

/// All devices with their site, to find the one just written by its id.
#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "QueryAuthenticated")]
pub struct NewDeviceDevices {
    pub topology: NewDeviceTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "TopologyHolder")]
pub struct NewDeviceTopology {
    pub all_devices: Vec<NewDeviceDevice>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "DeviceAccess")]
pub struct NewDeviceDevice {
    pub id: i32,
    pub name: String,
    pub site: Option<SiteId>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(schema = "authenticated", graphql_type = "SiteAccess")]
pub struct SiteId {
    pub id: i32,
}
