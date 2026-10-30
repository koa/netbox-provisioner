//! What is missing in NetBox or doesn't fit together, computed from the topology alone (the
//! Befund „NetBox in sich“ of `docs/config-helper.md`). Only the devices with RouterOS
//! are looked at, the ones the provisioner manages. Data only: the frontend words it and links
//! each object to NetBox.
use crate::{
    device::ros::{DEFAULT_MTU, dhcp_pool_v4},
    topology::{
        DeviceId, PortType, PrefixPool, SiteId, Topology,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess, vlan::VlanAccess, wlan_group::WlanGroupAccess,
        },
        interfaces::InterfaceTask,
        vlan_domain,
    },
};
use async_graphql::{Enum, SimpleObject, Union};
use ipnet::IpNet;
use std::sync::Arc;

/// How bad a finding is; ordered from the worst.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Enum)]
pub enum Severity {
    /// The generated config does something wrong or nothing at all.
    Error,
    /// Works, but is not as `docs/netbox-modeling.md` wants it, or a helper can't work with it.
    Warning,
    /// May be intended (a customer port), worth a look.
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct Finding {
    pub severity: Severity,
    /// The devices it concerns (both ends of a cable); none for the whole network.
    #[graphql(skip)]
    pub devices: Box<[DeviceId]>,
    pub kind: FindingKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Union)]
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
    RadioWithoutRole(RadioWithoutRole),
    LoopbackWithoutOspf(LoopbackWithoutOspf),
    VlanWithSeveralGateways(VlanWithSeveralGateways),
    VlanWithoutGateway(VlanWithoutGateway),
    TaggedVlanOnPort(TaggedVlanOnPort),
    RadioPathIncomplete(RadioPathIncomplete),
    LabelNotFarEnd(LabelNotFarEnd),
    VlanWithoutPort(VlanWithoutPort),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum IpVersion {
    V4,
    V6,
}

/// A NetBox device, by id (for the link) and name.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceRef {
    pub id: u32,
    pub name: Box<str>,
    /// its site, for the link to its page in the provisioner
    pub site: Option<u32>,
}
/// A NetBox interface and its device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceRef {
    pub id: u32,
    pub name: Box<str>,
    pub device: DeviceRef,
}
/// A NetBox IP address.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct AddressRef {
    pub id: u32,
    /// with its prefix length, `172.17.1.1/30`
    pub address: Box<str>,
}

/// No prefix has the tag `pool` for `version`: its helper has nothing to hand out from.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoolMissing {
    pub pool: PrefixPool,
    pub version: IpVersion,
}
/// Less than a tenth of the pool is free (addresses for `loopback`, `/32`s for `transfer`).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoolNearlyFull {
    pub pool: PrefixPool,
    pub version: IpVersion,
    pub free: u64,
    pub size: u64,
}
/// The device has no interface `lo` (type virtual) for its loopback.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NoLoopback {
    pub device: DeviceRef,
}
/// The device has no primary IPv4: no OSPF router ID, no VXLAN endpoint, no address from
/// NetBox to reach it at.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NoPrimaryIpv4 {
    pub device: DeviceRef,
}
/// The primary IPv4 is on `lo`, but no interface has `ospf`: nothing announces it, so the
/// provisioner, which connects to it, doesn't reach the device (a radio bridge that has lost
/// its DHCP address to it).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LoopbackWithoutOspf {
    pub device: DeviceRef,
    pub address: AddressRef,
}
/// The primary IPv4 is not on `lo` (on `interface`, or on none).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PrimaryIpv4NotOnLoopback {
    pub device: DeviceRef,
    pub address: AddressRef,
    pub interface: Option<InterfaceRef>,
}
/// No prefix of the address' own length exists; `dhcp`: the interface has the tag `dhcp`, whose
/// server needs it.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct AddressWithoutPrefix {
    pub interface: InterfaceRef,
    pub address: AddressRef,
    pub dhcp: bool,
}
/// The interface has the tag `dhcp`, but the prefix of `address` (`prefix`) has no address
/// left for a pool (a /30 with both hosts taken, a /31), or `address` is a /32 without one (a
/// loopback, router-stall-burri): no DHCP server there.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DhcpPoolEmpty {
    pub interface: InterfaceRef,
    pub address: AddressRef,
    pub prefix: Option<PrefixRef>,
}
/// A NetBox prefix.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PrefixRef {
    pub id: u32,
    /// `172.17.1.0/30`
    pub prefix: Box<str>,
}
/// A physical interface (Ethernet or radio) whose name is no RouterOS port name, so the
/// generator doesn't take it for a port.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortLikeName {
    pub interface: InterfaceRef,
}
/// An end of a wireless link without RF role (AP or station): the provisioner doesn't set up
/// the radio bridge, the radio keeps the configuration it has.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RadioWithoutRole {
    pub interface: InterfaceRef,
}
/// The interface has a VLAN (NetBox id `vlan`) that is in no VLAN group, so the provisioner
/// doesn't read it.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct VlanWithoutGroup {
    pub interface: InterfaceRef,
    pub vlan: u32,
}
/// The ends of a cable have different MTUs: OSPF doesn't come up between them.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct MtuMismatch {
    pub interface: InterfaceRef,
    pub mtu: u16,
    pub far_end: InterfaceRef,
    pub far_mtu: u16,
}
/// Only `interface` (the port, or its bridge) of a cable has the tag `ospf`, `far_end` not,
/// both with IPv4 addresses.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct OspfOnOneEnd {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}
/// Both ends of a cable have IPv4 addresses (on the port or its bridge), but none in a common
/// network.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NoCommonSubnet {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}
/// A cable between two routers with addresses in a common network, but `ospf` on neither end.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct AddressWithoutOspf {
    pub interface: InterfaceRef,
    pub far_end: InterfaceRef,
}

/// A NetBox VLAN.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct VlanRef {
    pub id: u32,
    pub vid: u16,
    pub name: Box<str>,
}
/// More than one device has an address in the VLAN within one domain (`topology::vlan_domain`):
/// two gateways in one network. A switch giving the VLAN on L2 only has no address in it.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct VlanWithSeveralGateways {
    pub vlan: VlanRef,
    /// the interfaces with the addresses, on different devices
    pub interfaces: Vec<InterfaceRef>,
}
/// No device has an address in the VLAN within one domain: its clients reach nothing. Not for
/// the management VLAN of a WLAN group, which CAPsMAN uses on L2.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct VlanWithoutGateway {
    pub vlan: VlanRef,
    /// the interfaces with the VLAN untagged in that domain
    pub interfaces: Vec<InterfaceRef>,
}
/// An Ethernet port carries tagged VLANs (its own, or its bridge's): not wanted, a VLAN goes to
/// another device over VXLAN (`vxlan_vni` of its group), own APs and switches over a direct
/// link.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct TaggedVlanOnPort {
    pub interface: InterfaceRef,
    pub vlans: Vec<VlanRef>,
}
/// A VLAN interface with a network whose VLAN reaches nobody: no other interface of the device
/// carries it (port, bridge), no WLAN has it, and no VXLAN or cable joins another device with it.
/// „VLAN am Port“ puts it on a port.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct VlanWithoutPort {
    pub interface: InterfaceRef,
    pub vlan: VlanRef,
}
/// A part of a radio link NetBox doesn't have.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum RadioPathPart {
    /// the cable from the bridge device to a router
    RouterCable,
    /// the wireless link to the other bridge device
    WirelessLink,
    /// the SSID of the wireless link
    Ssid,
    /// the cable from the other bridge device to a router
    PeerRouterCable,
}
/// A radio link from `radio` (a bridge device's) lacks `missing` in NetBox: the provisioner
/// can't set up the way between the two routers.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RadioPathIncomplete {
    pub radio: InterfaceRef,
    pub missing: Vec<RadioPathPart>,
}
/// The label of a port cabled to a RouterOS device doesn't name it (`None`: no label). The
/// generated port name carries it (`e02-ap1`), and „Verbindung“ sets it so.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LabelNotFarEnd {
    pub interface: InterfaceRef,
    pub label: Option<Box<str>>,
    pub far_end: InterfaceRef,
}

/// A WLAN of `group` without a VLAN: its CAPs forward the clients themselves into the bridge
/// of the WLAN's VLAN, which carries them over the VXLAN to their network, so without one the
/// clients connect and reach nothing (no bridge, no VXLAN on the CAPs).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanWithoutVlan {
    /// NetBox id of the Wireless LAN
    pub id: u32,
    pub ssid: Box<str>,
    pub group: WlanGroupRef,
}
/// A NetBox Wireless LAN Group.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanGroupRef {
    pub id: u32,
    pub name: Box<str>,
}

impl Topology {
    /// Every finding, the worst first.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }
    /// The findings about the devices of `site`.
    pub fn site_findings(&self, site: SiteId) -> Vec<Finding> {
        let devices = self.sites.get(&site).map(|d| &d.devices[..]).unwrap_or_default();
        self.findings
            .iter()
            .filter(|finding| finding.devices.iter().any(|d| devices.contains(d)))
            .cloned()
            .collect()
    }
    /// The topology with its findings, which are computed through the access types on an
    /// `Arc` of it.
    pub(super) fn with_findings(self) -> Topology {
        let topology = Arc::new(self);
        let findings = collect(&topology);
        let mut topology =
            Arc::into_inner(topology).expect("no access to the topology left after collect");
        topology.findings = findings;
        topology
    }
}

fn collect(topo: &Arc<Topology>) -> Box<[Finding]> {
    let mut findings = Vec::new();
    pools(topo, &mut findings);
    wlans(topo, &mut findings);
    vlans(topo, &mut findings);
    let mut devices = topo
        .list_devices()
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.name().cmp(b.name()));
    for device in &devices {
        device_findings(device, &mut findings);
    }
    findings.sort_by_key(|finding| finding.severity);
    findings.into_boxed_slice()
}

fn pools(topo: &Topology, findings: &mut Vec<Finding>) {
    for (pool, version) in [
        (PrefixPool::Loopback, IpVersion::V4),
        (PrefixPool::Loopback, IpVersion::V6),
        (PrefixPool::Transfer, IpVersion::V4),
    ] {
        let prefixes = topo
            .ip_prefixes
            .values()
            .filter(|p| p.pool == Some(pool) && version_of(&p.prefix) == version)
            .collect::<Vec<_>>();
        if prefixes.is_empty() {
            findings.push(Finding {
                severity: Severity::Warning,
                devices: Box::new([]),
                kind: FindingKind::PoolMissing(PoolMissing { pool, version }),
            });
            continue;
        }
        let mut size = 0u128;
        let mut used = 0u128;
        for prefix in prefixes {
            size = size.saturating_add(host_count(&prefix.prefix));
            used = used.saturating_add(match pool {
                PrefixPool::Loopback => topo
                    .ip_addresses
                    .values()
                    .filter(|a| prefix.prefix.contains(&a.ip.addr()))
                    .count() as u128,
                PrefixPool::Transfer => prefix
                    .children
                    .iter()
                    .filter_map(|child| topo.ip_prefixes.get(child))
                    .map(|child| host_count(&child.prefix))
                    .sum(),
            });
        }
        let free = size.saturating_sub(used);
        if free * 10 < size {
            findings.push(Finding {
                severity: Severity::Warning,
                devices: Box::new([]),
                kind: FindingKind::PoolNearlyFull(PoolNearlyFull {
                    pool,
                    version,
                    free: u64::try_from(free).unwrap_or(u64::MAX),
                    size: u64::try_from(size).unwrap_or(u64::MAX),
                }),
            });
        }
    }
}

/// The WLANs without a VLAN, at the group's controller and APs.
fn wlans(topo: &Arc<Topology>, findings: &mut Vec<Finding>) {
    let mut groups = topo
        .wlan_groups
        .keys()
        .map(|id| WlanGroupAccess::create(topo.clone(), *id))
        .collect::<Vec<_>>();
    groups.sort_by(|a, b| a.name().cmp(&b.name()));
    for group in groups {
        let devices = group
            .controller()
            .into_iter()
            .chain(group.aps())
            .filter(DeviceAccess::has_routeros)
            .map(|device| device.id())
            .collect::<Box<[_]>>();
        let mut wlans = group.wlan().filter(|wlan| wlan.vlan().is_none()).collect::<Vec<_>>();
        wlans.sort_by(|a, b| a.ssid().cmp(&b.ssid()));
        for wlan in wlans {
            findings.push(Finding {
                severity: Severity::Error,
                devices: devices.clone(),
                kind: FindingKind::WlanWithoutVlan(WlanWithoutVlan {
                    id: wlan.id().0,
                    ssid: wlan.ssid().unwrap_or_default().into(),
                    group: WlanGroupRef {
                        id: group.id().0,
                        name: group.name().unwrap_or_default().into(),
                    },
                }),
            });
        }
    }
}

/// One gateway per VLAN domain.
fn vlans(topo: &Arc<Topology>, findings: &mut Vec<Finding>) {
    let mut vlans = topo.list_vlans().collect::<Vec<_>>();
    vlans.sort_by_key(|vlan| (vlan.vlan_id(), vlan.id.0));
    for vlan in vlans {
        let Some(vlan_ref) = vlan_ref(&vlan) else { continue };
        for domain in vlan_domain::domains(&vlan) {
            let devices = domain
                .iter()
                .filter_map(|id| topo.get_device_by_id(id))
                .collect::<Vec<_>>();
            let gateways = devices
                .iter()
                .map(|device| (device, vlan_domain::l3_interfaces(&vlan, device)))
                .filter(|(_, l3)| !l3.is_empty())
                .collect::<Vec<_>>();
            // CAPsMAN finds its CAPs in a management VLAN on L2, it needs no gateway
            let management = topo.wlan_groups.values().any(|g| g.mgmt_vlan == Some(vlan.id));
            match gateways.len() {
                0 if management => {}
                0 => {
                    let interfaces = devices
                        .iter()
                        .flat_map(|device| device.interfaces().into_vec())
                        .filter(|i| i.untagged_vlan().map(|v| v.id) == Some(vlan.id))
                        .map(|i| interface_ref(&i))
                        .collect::<Vec<_>>();
                    findings.push(Finding {
                        severity: Severity::Warning,
                        devices: domain.iter().copied().collect(),
                        kind: FindingKind::VlanWithoutGateway(VlanWithoutGateway {
                            vlan: vlan_ref.clone(),
                            interfaces,
                        }),
                    });
                }
                1 => {}
                _ => findings.push(Finding {
                    severity: Severity::Error,
                    devices: gateways.iter().map(|(device, _)| device.id()).collect(),
                    kind: FindingKind::VlanWithSeveralGateways(VlanWithSeveralGateways {
                        vlan: vlan_ref.clone(),
                        interfaces: gateways
                            .iter()
                            .flat_map(|(_, l3)| l3.iter().map(interface_ref))
                            .collect(),
                    }),
                }),
            }
        }
    }
}

pub(crate) fn vlan_ref(vlan: &VlanAccess) -> Option<VlanRef> {
    Some(VlanRef {
        id: vlan.id.0,
        vid: vlan.vlan_id()?,
        name: vlan.name().unwrap_or_default().into(),
    })
}

/// A label naming the device: one contains the other, compared without case and anything but
/// letters and digits (`AP1` names `lab-ap1`, `SXT A` names `lab-sxt-a`).
pub(crate) fn names(label: &str, device: &str) -> bool {
    let plain = |text: &str| {
        text.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect::<String>()
    };
    let (label, device) = (plain(label), plain(device));
    !label.is_empty() && (device.contains(&label) || label.contains(&device))
}

fn device_findings(device: &DeviceAccess, findings: &mut Vec<Finding>) {
    let topo = device.topology();
    let mut push = |severity, devices: &[DeviceId], kind| {
        findings.push(Finding {
            severity,
            devices: devices.into(),
            kind,
        })
    };
    let this = [device.id()];
    let interfaces = device.interfaces();
    if !interfaces
        .iter()
        .any(|i| i.name() == "lo" && port_type(i) == Some(PortType::Loopback))
    {
        push(
            // a radio bridge device is reached at its loopback only
            if device.is_bridge_device() {
                Severity::Error
            } else {
                Severity::Warning
            },
            &this,
            FindingKind::NoLoopback(NoLoopback {
                device: device_ref(device),
            }),
        );
    }
    match device.primary_ip_v4() {
        // OSPF takes its router ID from it; without OSPF it is only the address to reach it at
        None => push(
            if interfaces.iter().any(InterfaceAccess::use_ospf) {
                Severity::Error
            } else {
                Severity::Warning
            },
            &this,
            FindingKind::NoPrimaryIpv4(NoPrimaryIpv4 {
                device: device_ref(device),
            }),
        ),
        Some(address) => {
            let interface = address.interface();
            if interface.as_ref().map(InterfaceAccess::name) == Some("lo")
                && !interfaces.iter().any(InterfaceAccess::use_ospf)
            {
                push(
                    Severity::Error,
                    &this,
                    FindingKind::LoopbackWithoutOspf(LoopbackWithoutOspf {
                        device: device_ref(device),
                        address: address_ref(&address),
                    }),
                );
            }
            if interface.as_ref().map(InterfaceAccess::name) != Some("lo") {
                push(
                    Severity::Warning,
                    &this,
                    FindingKind::PrimaryIpv4NotOnLoopback(PrimaryIpv4NotOnLoopback {
                        device: device_ref(device),
                        address: address_ref(&address),
                        interface: interface.as_ref().map(interface_ref),
                    }),
                )
            }
        }
    }
    let mut interfaces = interfaces.into_vec();
    interfaces.sort_by(|a, b| a.name().cmp(b.name()));
    for interface in &interfaces {
        // the interface's one DHCP server serves the networks with a pool, a network without
        // one (the `/31` to the router at the other end of a radio bridge without RouterOS)
        // only needs no clients
        let serves = interface.ips().iter().any(|address| {
            address.prefix().is_some_and(|prefix| {
                matches!(prefix.prefix(), Some(IpNet::V4(net)) if !dhcp_pool_v4(&prefix, net).is_empty())
            })
        });
        for address in interface.ips() {
            let Some(net) = address.net() else { continue };
            // a /32 or /128 (a loopback) needs no prefix of its own
            if net.prefix_len() < net.max_prefix_len() && address.prefix().is_none() {
                let dhcp = interface.is_enable_dhcp_server();
                push(
                    if dhcp {
                        Severity::Error
                    } else {
                        Severity::Warning
                    },
                    &this,
                    FindingKind::AddressWithoutPrefix(AddressWithoutPrefix {
                        interface: interface_ref(interface),
                        address: address_ref(&address),
                        dhcp,
                    }),
                );
            }
            if interface.is_enable_dhcp_server()
                && !serves
                && let IpNet::V4(v4) = net
            {
                let empty = match address.prefix() {
                    Some(prefix) => match prefix.prefix() {
                        Some(IpNet::V4(prefix_net)) if dhcp_pool_v4(&prefix, prefix_net).is_empty() => {
                            Some(Some(PrefixRef {
                                id: prefix.id().0,
                                prefix: prefix_net.to_string().into(),
                            }))
                        }
                        _ => None,
                    },
                    None => (v4.prefix_len() == 32).then_some(None),
                };
                if let Some(prefix) = empty {
                    push(
                        Severity::Error,
                        &this,
                        FindingKind::DhcpPoolEmpty(DhcpPoolEmpty {
                            interface: interface_ref(interface),
                            address: address_ref(&address),
                            prefix,
                        }),
                    );
                }
            }
        }
        if matches!(
            port_type(interface),
            Some(PortType::Ethernet | PortType::Wireless)
        ) && interface.external_port().is_none()
        {
            push(
                Severity::Warning,
                &this,
                FindingKind::PortLikeName(PortLikeName {
                    interface: interface_ref(interface),
                }),
            );
        }
        if interface.has_wireless_link() && interface.radio().is_none() {
            push(
                Severity::Warning,
                &this,
                FindingKind::RadioWithoutRole(RadioWithoutRole {
                    interface: interface_ref(interface),
                }),
            );
        }
        if port_type(interface) == Some(PortType::Ethernet) {
            let tagged = match interface.tagged_vlans().collect::<Vec<_>>() {
                own if !own.is_empty() => own,
                _ if interface.untagged_vlan().is_none() => interface
                    .bridge()
                    .map(|bridge| bridge.tagged_vlans().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            if !tagged.is_empty() {
                push(
                    Severity::Warning,
                    &this,
                    FindingKind::TaggedVlanOnPort(TaggedVlanOnPort {
                        interface: interface_ref(interface),
                        vlans: tagged.iter().filter_map(vlan_ref).collect(),
                    }),
                );
            }
        }
        if let Some(far_end) = interface
            .connected_interfaces()
            .into_iter()
            .find(|far| far.device().is_some_and(|d| d.has_routeros() && d.id() != device.id()))
            && let Some(far_device) = far_end.device()
            && !interface.label().is_some_and(|label| names(label, far_device.name()))
        {
            push(
                Severity::Hint,
                &this,
                FindingKind::LabelNotFarEnd(LabelNotFarEnd {
                    interface: interface_ref(interface),
                    label: interface.label().map(Into::into),
                    far_end: interface_ref(&far_end),
                }),
            );
        }
        if port_type(interface).is_none()
            && !interface.is_bridge()
            && !interface.ips().is_empty()
            && let Some(vlan) = interface.untagged_vlan()
            && let Some(vlan_ref) = vlan_ref(&vlan)
            && vlan.wlan().next().is_none()
            && !interfaces.iter().any(|other| {
                other.id() != interface.id() && other.carried_vlans().iter().any(|v| v.id == vlan.id)
            })
            && vlan_domain::domain_of(&vlan, device).len() <= 1
        {
            push(
                Severity::Warning,
                &this,
                FindingKind::VlanWithoutPort(VlanWithoutPort {
                    interface: interface_ref(interface),
                    vlan: vlan_ref,
                }),
            );
        }
        if let Some(data) = interface.data() {
            for vlan in data.vlan.iter().chain(data.tagged_vlans.iter()) {
                if !topo.vlans.contains_key(vlan) {
                    push(
                        Severity::Warning,
                        &this,
                        FindingKind::VlanWithoutGroup(VlanWithoutGroup {
                            interface: interface_ref(interface),
                            vlan: vlan.0,
                        }),
                    );
                }
            }
        }
        for far_end in interface.connected_interfaces() {
            let Some(far_device) = far_end.device().filter(DeviceAccess::has_routeros) else {
                continue;
            };
            // both ends are looked at, report it from one
            if far_end.id() < interface.id() {
                continue;
            }
            let both = [device.id(), far_device.id()];
            let mtu = interface.mtu().unwrap_or(DEFAULT_MTU);
            let far_mtu = far_end.mtu().unwrap_or(DEFAULT_MTU);
            if mtu != far_mtu {
                push(
                    Severity::Error,
                    &both,
                    FindingKind::MtuMismatch(MtuMismatch {
                        interface: interface_ref(interface),
                        mtu,
                        far_end: interface_ref(&far_end),
                        far_mtu,
                    }),
                );
            }
            let near = interface.bridge().unwrap_or_else(|| interface.clone());
            let far = far_end.bridge().unwrap_or_else(|| far_end.clone());
            let near_nets = ipv4_nets(&near);
            let far_nets = ipv4_nets(&far);
            // an end without address is L2 only (a bridge device), OSPF doesn't stop there
            if near_nets.is_empty() || far_nets.is_empty() {
                continue;
            }
            let common = near_nets
                .iter()
                .any(|a| far_nets.iter().any(|b| a.trunc() == b.trunc()));
            if near.use_ospf() != far.use_ospf() {
                let (with, without) = if near.use_ospf() {
                    (&near, &far)
                } else {
                    (&far, &near)
                };
                push(
                    Severity::Error,
                    &both,
                    FindingKind::OspfOnOneEnd(OspfOnOneEnd {
                        interface: interface_ref(with),
                        far_end: interface_ref(without),
                    }),
                );
            }
            if !common {
                push(
                    Severity::Error,
                    &both,
                    FindingKind::NoCommonSubnet(NoCommonSubnet {
                        interface: interface_ref(&near),
                        far_end: interface_ref(&far),
                    }),
                );
            } else if !near.use_ospf() && !far.use_ospf() {
                push(
                    Severity::Hint,
                    &both,
                    FindingKind::AddressWithoutOspf(AddressWithoutOspf {
                        interface: interface_ref(&near),
                        far_end: interface_ref(&far),
                    }),
                );
            }
        }
    }
    if device.is_bridge_device() {
        radio_path(device, findings);
    }
}

/// What a bridge device's radio link lacks, said once per link: from the end with the lower
/// id, or from this one where the other is missing.
fn radio_path(device: &DeviceAccess, findings: &mut Vec<Finding>) {
    for (row, _) in device.interface_rows() {
        let Some(path) = row.radio_path.filter(|_| row.task == InterfaceTask::RadioLink) else {
            continue;
        };
        let Some(radio) = path.radio.clone() else { continue };
        if path.peer_radio.as_ref().is_some_and(|peer| peer.device.id < device.id().0) {
            continue;
        }
        let missing = [
            (path.router_port.is_none(), RadioPathPart::RouterCable),
            (path.peer_radio.is_none(), RadioPathPart::WirelessLink),
            (path.peer_radio.is_some() && path.ssid.is_none(), RadioPathPart::Ssid),
            (
                path.peer_radio.is_some() && path.peer_router_port.is_none(),
                RadioPathPart::PeerRouterCable,
            ),
        ]
        .into_iter()
        .filter_map(|(missing, part)| missing.then_some(part))
        .collect::<Vec<_>>();
        if missing.is_empty() {
            continue;
        }
        let devices = [
            Some(device.id().0),
            path.router_port.as_ref().map(|p| p.device.id),
            path.peer_radio.as_ref().map(|p| p.device.id),
            path.peer_router_port.as_ref().map(|p| p.device.id),
        ]
        .into_iter()
        .flatten()
        .map(DeviceId)
        .collect::<std::collections::BTreeSet<_>>();
        findings.push(Finding {
            severity: Severity::Warning,
            devices: devices.into_iter().collect(),
            kind: FindingKind::RadioPathIncomplete(RadioPathIncomplete { radio, missing }),
        });
    }
}

fn port_type(interface: &InterfaceAccess) -> Option<PortType> {
    interface.data().and_then(|d| d.port_type)
}

fn ipv4_nets(interface: &InterfaceAccess) -> Vec<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(IpAddressAccess::net)
        .filter(|net| matches!(net, IpNet::V4(_)))
        .collect()
}

fn version_of(net: &IpNet) -> IpVersion {
    match net {
        IpNet::V4(_) => IpVersion::V4,
        IpNet::V6(_) => IpVersion::V6,
    }
}

/// How many addresses `net` spans.
fn host_count(net: &IpNet) -> u128 {
    1u128
        .checked_shl(u32::from(net.max_prefix_len() - net.prefix_len()))
        .unwrap_or(u128::MAX)
}

pub(crate) fn device_ref(device: &DeviceAccess) -> DeviceRef {
    DeviceRef {
        id: device.id().0,
        name: device.name().into(),
        site: device.data().and_then(|d| d.site).map(|site| site.0),
    }
}

pub(crate) fn interface_ref(interface: &InterfaceAccess) -> InterfaceRef {
    InterfaceRef {
        id: interface.id().0,
        name: interface.name().into(),
        device: interface
            .device()
            .as_ref()
            .map(device_ref)
            .unwrap_or(DeviceRef {
                id: 0,
                name: Box::default(),
                site: None,
            }),
    }
}

pub(crate) fn address_ref(address: &IpAddressAccess) -> AddressRef {
    AddressRef {
        id: address.id().0,
        address: address
            .net()
            .map(|net| net.to_string())
            .unwrap_or_default()
            .into(),
    }
}

#[cfg(test)]
mod test;
