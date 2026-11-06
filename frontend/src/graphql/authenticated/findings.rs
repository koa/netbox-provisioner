//! The findings about NetBox itself (`TopologyHolder.findings`, `components::findings`); the
//! interface view groups the same type onto its rows (`super::interfaces`).
use super::{
    helper::Helper,
    schema::schema,
    types::{
        AddressRef, DeviceRef, InterfaceRef, IpVersion, PrefixRef, ServiceRef, Severity, SiteRef,
        VlanRef, WideInt, WlanGroupRef,
    },
};

#[derive(cynic::QueryVariables, Debug)]
pub struct FindingsVariables {
    pub site: Option<i32>,
    pub device: Option<i32>,
    pub after: Option<Helper>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "SubscriptionAuthenticated",
    variables = "FindingsVariables"
)]
pub struct Findings {
    pub topology: FindingsTopology,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(
    schema = "authenticated",
    graphql_type = "TopologyHolder",
    variables = "FindingsVariables"
)]
pub struct FindingsTopology {
    pub netbox_url: String,
    #[arguments(siteId: $site, deviceId: $device, after: $after)]
    pub findings: Vec<Finding>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct Finding {
    pub severity: Severity,
    pub kind: FindingKind,
}

/// One this frontend doesn't know yet is `Unknown`.
#[derive(cynic::InlineFragments, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub enum FindingKind {
    PoolMissing(PoolMissing),
    PoolNearlyFull(PoolNearlyFull),
    NoLoopback(NoLoopback),
    NoPrimaryIpv4(NoPrimaryIpv4),
    PrimaryIpv4NotOnLoopback(PrimaryIpv4NotOnLoopback),
    AddressWithoutPrefix(AddressWithoutPrefix),
    DhcpPoolEmpty(DhcpPoolEmpty),
    PortLikeName(PortLikeName),
    VlanWithoutGroup(VlanWithoutGroup),
    MtuMismatch(MtuMismatch),
    OspfOnOneEnd(OspfOnOneEnd),
    NoCommonSubnet(NoCommonSubnet),
    AddressWithoutOspf(AddressWithoutOspf),
    WlanWithoutVlan(WlanWithoutVlan),
    WlanGroupWithoutController(WlanGroupWithoutController),
    RadioWithoutRole(RadioWithoutRole),
    NatWithoutInternet(NatWithoutInternet),
    LoopbackWithoutOspf(LoopbackWithoutOspf),
    NoCountry(NoCountry),
    VlanWithSeveralGateways(VlanWithSeveralGateways),
    VlanWithoutGateway(VlanWithoutGateway),
    TaggedVlanOnPort(TaggedVlanOnPort),
    RadioPathIncomplete(RadioPathIncomplete),
    LabelNotFarEnd(LabelNotFarEnd),
    VlanWithoutPort(VlanWithoutPort),
    ForwardWithoutRouter(ForwardWithoutRouter),
    ForwardWithoutSite(ForwardWithoutSite),
    ForwardPortTaken(ForwardPortTaken),
    ForwardIncomplete(ForwardIncomplete),
    #[cynic(fallback)]
    Unknown,
}

impl FindingKind {
    /// Its GraphQL type (`NoLoopback`), as `i18n::helper::finding_kind` takes it; `Unknown` for
    /// one this frontend doesn't know.
    pub fn name(&self) -> &'static str {
        match self {
            FindingKind::PoolMissing(_) => "PoolMissing",
            FindingKind::PoolNearlyFull(_) => "PoolNearlyFull",
            FindingKind::NoLoopback(_) => "NoLoopback",
            FindingKind::NoPrimaryIpv4(_) => "NoPrimaryIpv4",
            FindingKind::PrimaryIpv4NotOnLoopback(_) => "PrimaryIpv4NotOnLoopback",
            FindingKind::AddressWithoutPrefix(_) => "AddressWithoutPrefix",
            FindingKind::DhcpPoolEmpty(_) => "DhcpPoolEmpty",
            FindingKind::PortLikeName(_) => "PortLikeName",
            FindingKind::VlanWithoutGroup(_) => "VlanWithoutGroup",
            FindingKind::MtuMismatch(_) => "MtuMismatch",
            FindingKind::OspfOnOneEnd(_) => "OspfOnOneEnd",
            FindingKind::NoCommonSubnet(_) => "NoCommonSubnet",
            FindingKind::AddressWithoutOspf(_) => "AddressWithoutOspf",
            FindingKind::WlanWithoutVlan(_) => "WlanWithoutVlan",
            FindingKind::WlanGroupWithoutController(_) => "WlanGroupWithoutController",
            FindingKind::RadioWithoutRole(_) => "RadioWithoutRole",
            FindingKind::NatWithoutInternet(_) => "NatWithoutInternet",
            FindingKind::LoopbackWithoutOspf(_) => "LoopbackWithoutOspf",
            FindingKind::NoCountry(_) => "NoCountry",
            FindingKind::VlanWithSeveralGateways(_) => "VlanWithSeveralGateways",
            FindingKind::VlanWithoutGateway(_) => "VlanWithoutGateway",
            FindingKind::TaggedVlanOnPort(_) => "TaggedVlanOnPort",
            FindingKind::RadioPathIncomplete(_) => "RadioPathIncomplete",
            FindingKind::LabelNotFarEnd(_) => "LabelNotFarEnd",
            FindingKind::VlanWithoutPort(_) => "VlanWithoutPort",
            FindingKind::ForwardWithoutRouter(_) => "ForwardWithoutRouter",
            FindingKind::ForwardWithoutSite(_) => "ForwardWithoutSite",
            FindingKind::ForwardPortTaken(_) => "ForwardPortTaken",
            FindingKind::ForwardIncomplete(_) => "ForwardIncomplete",
            FindingKind::Unknown => "Unknown",
        }
    }
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoolMissing {
    pub pool: PrefixPool,
    pub version: IpVersion,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PoolNearlyFull {
    pub pool: PrefixPool,
    pub version: IpVersion,
    pub free: WideInt,
    pub size: WideInt,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NoLoopback {
    pub device: DeviceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NoPrimaryIpv4 {
    pub device: DeviceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PrimaryIpv4NotOnLoopback {
    pub device: DeviceRef,
    pub address: AddressRef,
    pub interface: Option<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct AddressWithoutPrefix {
    pub interface: InterfaceRef,
    pub address: AddressRef,
    pub dhcp: bool,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct DhcpPoolEmpty {
    pub interface: InterfaceRef,
    pub address: AddressRef,
    pub prefix: Option<PrefixRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct PortLikeName {
    pub interface: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct VlanWithoutGroup {
    pub interface: InterfaceRef,
    pub vlan: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct MtuMismatch {
    pub interface: InterfaceRef,
    pub mtu: i32,
    pub far_end: InterfaceRef,
    pub far_mtu: i32,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct OspfOnOneEnd {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NoCommonSubnet {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct AddressWithoutOspf {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WlanWithoutVlan {
    pub id: i32,
    pub ssid: String,
    pub group: WlanGroupRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct WlanGroupWithoutController {
    pub group: WlanGroupRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RadioWithoutRole {
    pub interface: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NatWithoutInternet {
    pub interface: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LoopbackWithoutOspf {
    pub device: DeviceRef,
    pub address: AddressRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct NoCountry {
    pub device: DeviceRef,
    pub site: Option<SiteRef>,
    pub region: Option<String>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct VlanWithSeveralGateways {
    pub vlan: VlanRef,
    pub interfaces: Vec<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct VlanWithoutGateway {
    pub vlan: VlanRef,
    pub interfaces: Vec<InterfaceRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct TaggedVlanOnPort {
    pub interface: InterfaceRef,
    pub vlans: Vec<VlanRef>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct RadioPathIncomplete {
    pub radio: InterfaceRef,
    pub missing: Vec<RadioPathPart>,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct LabelNotFarEnd {
    pub interface: InterfaceRef,
    pub label: Option<String>,
    pub far_end: InterfaceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct VlanWithoutPort {
    pub interface: InterfaceRef,
    pub vlan: VlanRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ForwardWithoutRouter {
    pub service: ServiceRef,
    pub address: AddressRef,
    pub site: SiteRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ForwardWithoutSite {
    pub service: ServiceRef,
    pub address: AddressRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ForwardPortTaken {
    pub service: ServiceRef,
    pub port: String,
    pub router: DeviceRef,
    pub by: ServiceRef,
}

#[derive(cynic::QueryFragment, Debug, Clone, PartialEq)]
#[cynic(schema = "authenticated")]
pub struct ForwardIncomplete {
    pub service: ServiceRef,
}

/// The tag of a pool (`PrefixPool`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum PrefixPool {
    Loopback,
    Transfer,
    #[cynic(fallback)]
    Other,
}

/// What a radio bridge lacks (`RadioPathPart`).
#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
#[cynic(schema = "authenticated")]
pub enum RadioPathPart {
    RouterCable,
    WirelessLink,
    Ssid,
    PeerRouterCable,
    #[cynic(fallback)]
    Other,
}
