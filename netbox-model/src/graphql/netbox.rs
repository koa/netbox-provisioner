use crate::graphql::netbox::id::create_from_str;
use cynic::impl_scalar;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::net::IpAddr;
use std::ops::Deref;
use std::str::FromStr;

#[cynic::schema("netbox")]
mod schema {}
pub mod id;

id::create_id!(InterfaceId);
id::create_id!(DeviceId);
id::create_id!(VxlanId);
id::create_id!(VlanId);
id::create_id!(WlanId);
id::create_id!(VlanGroupId);
id::create_id!(WlanGroupId);
id::create_id!(FrontPortId);
id::create_id!(RearPortId);
id::create_id!(CableId);
id::create_id!(IpRangeId);
id::create_id!(IpAddressId);
id::create_id!(PrefixId);
id::create_id!(TenantId);

create_from_str!(IpNetWrapper, IpNet, "an ip address including netmask");
create_from_str!(IpWrapper, IpAddr, "an ip address without netmask");

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "VLANType")]
pub struct Vlantype {
    pub id: VlanId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct FetchTopology {
    #[cynic(rename = "device_list")]
    pub device_list: Vec<DeviceType>,
    #[cynic(rename = "tenant_list")]
    pub tenant_list: Vec<TenantType>,
    #[cynic(rename = "cable_list")]
    pub cable_list: Vec<CableType>,
    #[cynic(rename = "l2vpn_list")]
    pub l2vpn_list: Vec<VxLan>,
    #[cynic(rename = "wireless_lan_group_list")]
    pub wireless_lan_group_list: Vec<WlanGroup>,
    #[cynic(rename = "vlan_group_list")]
    pub vlan_group_list: Vec<VlanGroup>,
    #[cynic(rename = "ip_address_list")]
    pub ip_address_list: Vec<IPaddressType>,
    #[cynic(rename = "prefix_list")]
    pub prefix_list: Vec<PrefixType>,
    #[cynic(rename = "ip_range_list")]
    pub ip_range_list: Vec<IprangeType>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "VLANGroupType")]
pub struct VlanGroup {
    pub id: VlanGroupId,
    pub name: String,
    pub vlans: Vec<Vlan>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "VLANType")]
pub struct Vlan {
    pub id: VlanId,
    pub name: String,
    pub vid: i32,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "WirelessLANGroupType")]
pub struct WlanGroup {
    pub id: WlanGroupId,
    #[cynic(rename = "custom_fields")]
    pub custom_fields: WlanFields,
    //pub custom_field_data: Json,
    pub name: String,
    #[cynic(rename = "wireless_lans")]
    pub wireless_lans: Vec<Wlan>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "WirelessLANType")]
pub struct Wlan {
    pub id: WlanId,
    pub ssid: String,
    #[cynic(rename = "auth_type")]
    pub auth_type: Option<String>,
    #[cynic(rename = "auth_psk")]
    pub auth_psk: String,
    pub vlan: Option<VlanReference>,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct TenantType {
    pub id: TenantId,
    //#[cynic(rename = "custom_field_data")]
    //pub custom_field_data: Json,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct PrefixType {
    pub id: PrefixId,
    pub prefix: IpNetWrapper,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "L2VPNType")]
pub struct VxLan {
    pub id: VxlanId,
    pub name: String,
    #[cynic(rename = "type")]
    pub type_: String,
    pub identifier: Option<i32>,
    pub terminations: Vec<VxLanTermination>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "L2VPNTerminationType")]
pub struct VxLanTermination {
    #[cynic(rename = "assigned_object")]
    pub assigned_object: L2VpnassignmentType,
}

#[derive(cynic::QueryFragment, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceType {
    pub id: InterfaceId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "IPRangeType")]
pub struct IprangeType {
    pub id: IpRangeId,
    pub role: Option<RoleType>,
    #[cynic(rename = "start_address")]
    pub start_address: IpNetWrapper,
    #[cynic(rename = "end_address")]
    pub end_address: IpNetWrapper,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct RoleType {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "IPAddressType")]
pub struct IPaddressType {
    pub id: IpAddressId,
    pub address: IpNetWrapper,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct DeviceType {
    pub id: DeviceId,
    pub name: Option<String>,
    #[cynic(rename = "primary_ip4")]
    pub primary_ip4: Option<IpAddressReference>,
    #[cynic(rename = "primary_ip6")]
    pub primary_ip6: Option<IpAddressReference>,
    pub role: DeviceRoleType,
    pub interfaces: Vec<DeviceInterface>,
    pub frontports: Vec<DeviceFrontPort>,
    pub rearports: Vec<DeviceRearPort>,
    pub platform: Option<DevicePlatform>,
    pub tenant: Option<TenantReference>,
    pub location: Option<LocationReference>,
    pub site: SiteType,
    pub serial: String,
    #[cynic(rename = "custom_field_data")]
    pub custom_field_data: MikrotikCredentialsField,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct SiteType {
    pub tenant: Option<TenantReference>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "LocationType")]
pub struct LocationReference {
    pub tenant: Option<TenantReference>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "TenantType")]
pub struct TenantReference {
    pub id: TenantId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "PlatformType")]
pub struct DevicePlatform {
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "RearPortType")]
pub struct DeviceRearPort {
    pub id: RearPortId,
    pub name: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "FrontPortType")]
pub struct DeviceFrontPort {
    pub id: FrontPortId,
    pub name: String,
    #[cynic(rename = "rear_port")]
    pub rear_port: DeviceRearPortId,
}
#[derive(cynic::QueryFragment, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cynic(graphql_type = "FrontPortType")]
pub struct DeviceFrontPortId {
    pub id: FrontPortId,
}

#[derive(cynic::QueryFragment, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cynic(graphql_type = "RearPortType")]
pub struct DeviceRearPortId {
    pub id: RearPortId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "InterfaceType")]
pub struct DeviceInterface {
    pub id: InterfaceId,
    pub name: String,
    pub label: String,
    #[cynic(rename = "ip_addresses")]
    pub ip_addresses: Vec<IpAddressReference>,
    pub enabled: bool,
    #[cynic(rename = "type")]
    pub type_: String,
    pub bridge: Option<InterfaceReference>,
    pub tags: Vec<TagType>,
    #[cynic(rename = "untagged_vlan")]
    pub untagged_vlan: Option<VlanReference>,
    #[cynic(rename = "tagged_vlans")]
    pub tagged_vlans: Vec<VlanReference>,
    #[cynic(rename = "poe_mode")]
    pub poe_mode: Option<String>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "VLANType")]
pub struct VlanReference {
    pub id: VlanId,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct TagType {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "InterfaceType")]
pub struct InterfaceReference {
    pub id: InterfaceId,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "IPAddressType")]
pub struct IpAddressReference {
    pub id: IpAddressId,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct DeviceRoleType {
    pub slug: String,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct CableType {
    pub id: CableId,
    #[cynic(rename = "a_terminations")]
    pub a_terminations: Vec<CableTerminationTerminationType>,
    #[cynic(rename = "b_terminations")]
    pub b_terminations: Vec<CableTerminationTerminationType>,
}
#[derive(cynic::InlineFragments, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum CableTerminationTerminationType {
    ConnectedInterface(InterfaceType),
    ConnectedFrontPort(DeviceFrontPortId),
    ConnectedRearPort(DeviceRearPortId),
    #[cynic(fallback)]
    Other,
}

#[derive(cynic::InlineFragments, Debug)]
#[cynic(graphql_type = "L2VPNAssignmentType")]
pub enum L2VpnassignmentType {
    InterfaceType(InterfaceType),
    Vlantype(Vlantype),
    #[cynic(fallback)]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MikrotikCredentialsField {
    pub mikrotik_credentials: Option<Box<str>>,
}
impl_scalar!(MikrotikCredentialsField, schema::JSON);
#[derive(Debug, Deserialize, Serialize)]
pub struct WlanFields {
    pub wlan_group: Option<u32>,
    pub wlan_mgmt: Option<u32>,
    pub controller: Option<u32>,
}
impl_scalar!(WlanFields, schema::JSON);

#[cfg(test)]
mod tests;
