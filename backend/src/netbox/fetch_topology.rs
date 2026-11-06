//! What the topology is built from: one query over NetBox's GraphQL (`netbox::fetch_topology`),
//! checked against `schema.graphqls` (registered by `build.rs` as the default schema) when the
//! backend compiles. Kept as it came (`Topology.source`): a helper's preview applies its plan to
//! the JSON of it and builds the topology again from that, so each type serializes to what it
//! reads.
use super::{BigInt, Decimal, JSON};
use cynic::Id;
use serde::Serialize;

#[cynic::schema("netbox")]
mod schema {}

cynic::impl_scalar!(BigInt, schema::BigInt);
cynic::impl_scalar!(Decimal, schema::Decimal);
cynic::impl_scalar!(JSON, schema::JSON);

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "Query", rename_all = "None")]
pub struct FetchTopology {
    pub device_list: Vec<Device>,
    pub tenant_list: Vec<Tenant>,
    pub cable_list: Vec<Cable>,
    /// a cable path goes on from one end of a circuit (a leased line, a fiber between
    /// buildings) at its other end
    pub circuit_list: Vec<Circuit>,
    pub wireless_lan_group_list: Vec<WirelessLanGroup>,
    pub vlan_group_list: Vec<VlanGroup>,
    pub ip_address_list: Vec<IpAddress>,
    pub prefix_list: Vec<Prefix>,
    pub ip_range_list: Vec<IpRange>,
    /// the ones with tag `forward` open their ports from the internet (`docs/firewall.md`)
    pub service_list: Vec<Service>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "DeviceType", rename_all = "None")]
pub struct Device {
    pub id: Id,
    pub name: Option<String>,
    pub primary_ip4: Option<IpAddressRef>,
    pub primary_ip6: Option<IpAddressRef>,
    pub role: DeviceRole,
    pub device_type: DeviceTypeOf,
    pub interfaces: Vec<Interface>,
    pub frontports: Vec<FrontPort>,
    pub rearports: Vec<RearPort>,
    pub platform: Option<Platform>,
    pub tenant: Option<TenantRef>,
    pub location: Option<Location>,
    pub site: Site,
    pub serial: String,
    pub custom_field_data: JSON,
    pub config_context: JSON,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "DeviceRoleType", rename_all = "None")]
pub struct DeviceRole {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "DeviceTypeType", rename_all = "None")]
pub struct DeviceTypeOf {
    pub model: String,
    pub manufacturer: Manufacturer,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "ManufacturerType", rename_all = "None")]
pub struct Manufacturer {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "InterfaceType", rename_all = "None")]
pub struct Interface {
    pub id: Id,
    pub name: String,
    pub label: String,
    pub ip_addresses: Vec<IpAddressRef>,
    pub enabled: bool,
    #[cynic(rename = "type")]
    #[serde(rename = "type")]
    pub type_: String,
    pub mtu: Option<i32>,
    pub speed: Option<BigInt>,
    pub duplex: Option<String>,
    pub bridge: Option<InterfaceRef>,
    pub tags: Vec<Tag>,
    pub untagged_vlan: Option<VlanRef>,
    pub tagged_vlans: Vec<VlanRef>,
    pub poe_mode: Option<String>,
    pub poe_type: Option<String>,
    /// a lease client's (`device::ros::dhcp`); the MikroTiks' aren't kept in NetBox
    pub primary_mac_address: Option<MacAddress>,
    pub rf_role: Option<String>,
    pub rf_channel_frequency: Option<Decimal>,
    pub rf_channel_width: Option<Decimal>,
    pub tx_power: Option<i32>,
    pub wireless_link: Option<WirelessLink>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "MACAddressType", rename_all = "None")]
pub struct MacAddress {
    pub mac_address: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "WirelessLinkType", rename_all = "None")]
pub struct WirelessLink {
    pub id: Id,
    pub interface_a: InterfaceRef,
    pub interface_b: InterfaceRef,
    pub ssid: String,
    pub auth_type: Option<String>,
    pub auth_psk: String,
    pub custom_field_data: JSON,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "FrontPortType", rename_all = "None")]
pub struct FrontPort {
    pub id: Id,
    pub name: String,
    pub mappings: Vec<PortMapping>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "PortMappingType", rename_all = "None")]
pub struct PortMapping {
    pub rear_port: RearPortId,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "RearPortType", rename_all = "None")]
pub struct RearPort {
    pub id: Id,
    pub name: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "PlatformType", rename_all = "None")]
pub struct Platform {
    pub name: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "LocationType", rename_all = "None")]
pub struct Location {
    pub tenant: Option<TenantRef>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "SiteType", rename_all = "None")]
pub struct Site {
    pub id: Id,
    pub name: String,
    pub tenant: Option<TenantRef>,
    pub region: Option<Region>,
}

/// Up to six levels of a site's region hierarchy: the topmost one is the country.
#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "RegionType", rename_all = "None")]
pub struct Region {
    pub slug: String,
    #[cynic(recurse = "5")]
    pub parent: Option<Box<Region>>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "TenantType", rename_all = "None")]
pub struct Tenant {
    pub id: Id,
    pub custom_field_data: JSON,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "TenantType", rename_all = "None")]
pub struct TenantRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "CableType", rename_all = "None")]
pub struct Cable {
    pub id: Id,
    #[cynic(rename = "type")]
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub a_terminations: Vec<CableTermination>,
    pub b_terminations: Vec<CableTermination>,
}

/// What a cable ends at; the provisioner follows a cable over these only.
#[derive(cynic::InlineFragments, Serialize, Debug, Clone)]
#[cynic(graphql_type = "CableTerminationTerminationType")]
#[serde(tag = "__typename")]
pub enum CableTermination {
    #[serde(rename = "InterfaceType")]
    Interface(InterfaceRef),
    #[serde(rename = "FrontPortType")]
    FrontPort(FrontPortRef),
    #[serde(rename = "RearPortType")]
    RearPort(RearPortId),
    #[serde(rename = "CircuitTerminationType")]
    CircuitTermination(CircuitTerminationRef),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "InterfaceType", rename_all = "None")]
pub struct InterfaceRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "FrontPortType", rename_all = "None")]
pub struct FrontPortRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "RearPortType", rename_all = "None")]
pub struct RearPortId {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "CircuitTerminationType", rename_all = "None")]
pub struct CircuitTerminationRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "CircuitType", rename_all = "None")]
pub struct Circuit {
    pub cid: String,
    pub terminations: Vec<CircuitTerminationRef>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "WirelessLANGroupType", rename_all = "None")]
pub struct WirelessLanGroup {
    pub id: Id,
    pub custom_fields: JSON,
    pub name: String,
    pub wireless_lans: Vec<WirelessLan>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "WirelessLANType", rename_all = "None")]
pub struct WirelessLan {
    pub id: Id,
    pub ssid: String,
    pub auth_type: Option<String>,
    pub auth_psk: String,
    pub vlan: Option<VlanRef>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "VLANGroupType", rename_all = "None")]
pub struct VlanGroup {
    pub id: Id,
    pub name: String,
    pub custom_field_data: JSON,
    pub vlans: Vec<Vlan>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "VLANType", rename_all = "None")]
pub struct Vlan {
    pub id: Id,
    pub name: String,
    pub vid: i32,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "VLANType", rename_all = "None")]
pub struct VlanRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "IPAddressType", rename_all = "None")]
pub struct IpAddress {
    pub id: Id,
    pub address: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "IPAddressType", rename_all = "None")]
pub struct IpAddressRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "PrefixType", rename_all = "None")]
pub struct Prefix {
    pub id: Id,
    pub prefix: String,
    pub description: String,
    pub custom_field_data: JSON,
    pub tags: Vec<Tag>,
    pub scope: Option<PrefixScope>,
}

#[derive(cynic::InlineFragments, Serialize, Debug, Clone)]
#[cynic(graphql_type = "PrefixScopeType")]
#[serde(tag = "__typename")]
pub enum PrefixScope {
    #[serde(rename = "SiteType")]
    Site(SiteRef),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "SiteType", rename_all = "None")]
pub struct SiteRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "TagType", rename_all = "None")]
pub struct Tag {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "IPRangeType", rename_all = "None")]
pub struct IpRange {
    pub id: Id,
    pub role: Option<Role>,
    pub start_address: String,
    pub end_address: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "RoleType", rename_all = "None")]
pub struct Role {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "ServiceType", rename_all = "None")]
pub struct Service {
    pub id: Id,
    pub name: String,
    pub port_mappings: Vec<String>,
    pub tags: Vec<Tag>,
    pub ipaddresses: Vec<IpAddressRef>,
    pub parent: Option<ServiceParent>,
}

#[derive(cynic::InlineFragments, Serialize, Debug, Clone)]
#[cynic(graphql_type = "ServiceParentType")]
#[serde(tag = "__typename")]
pub enum ServiceParent {
    #[serde(rename = "DeviceType")]
    Device(DeviceRef),
    #[serde(rename = "VirtualMachineType")]
    VirtualMachine(VirtualMachine),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "DeviceType", rename_all = "None")]
pub struct DeviceRef {
    pub id: Id,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "VirtualMachineType", rename_all = "None")]
pub struct VirtualMachine {
    pub site: Option<SiteRef>,
    pub cluster: Option<Cluster>,
}

#[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
#[cynic(graphql_type = "ClusterType", rename_all = "None")]
pub struct Cluster {
    pub scope: Option<ClusterScope>,
}

#[derive(cynic::InlineFragments, Serialize, Debug, Clone)]
#[cynic(graphql_type = "ClusterScopeType")]
#[serde(tag = "__typename")]
pub enum ClusterScope {
    #[serde(rename = "SiteType")]
    Site(SiteRef),
    #[cynic(fallback)]
    Other,
}
