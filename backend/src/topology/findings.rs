//! What is missing in NetBox or doesn't fit together, computed from the topology alone (the
//! Befund „NetBox in sich“ of `docs/concepts/config-helper.md`). Only the devices with RouterOS
//! are looked at, the ones the provisioner manages. Data only: the frontend words it and links
//! each object to NetBox.
use crate::{
    device::ros::{DEFAULT_MTU, dhcp_pool_v4},
    topology::{
        DeviceId, PortType, PrefixPool, SiteId, Topology,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess, wlan_group::WlanGroupAccess,
        },
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
            Severity::Warning,
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
            }),
    }
}

fn address_ref(address: &IpAddressAccess) -> AddressRef {
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
mod test {
    use super::*;
    use crate::{
        device::ros::netbox_example_test::example_topology,
        topology::{
            Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, IpPrefixData,
            IpPrefixId, PhysicalPortId, VlanId, test::TopologyBuilder,
        },
    };
    use std::{collections::BTreeMap, str::FromStr};

    /// Two routers, each with `lo` (its primary IPv4) and `ether1`, cabled to each other with
    /// `ospf` and a /30; a `loopback` and a `transfer` pool with room.
    struct Net {
        builder: TopologyBuilder,
        r1: DeviceId,
        r2: DeviceId,
        e1: InterfaceId,
        e2: InterfaceId,
    }

    impl Net {
        fn new() -> Self {
            let mut builder = TopologyBuilder::default();
            let r1 = router(&mut builder, "r1", "172.16.1.1/32");
            let r2 = router(&mut builder, "r2", "172.16.1.2/32");
            let e1 = port(&mut builder, r1, "ether1", "172.17.1.1/30");
            let e2 = port(&mut builder, r2, "ether1", "172.17.1.2/30");
            let cable = builder.cables.next_id();
            builder.cables.insert(
                cable,
                Cable {
                    port_a: Box::new([CablePort::Interface(e1)]),
                    port_b: Box::new([CablePort::Interface(e2)]),
                    kind: None,
                },
            );
            pool(&mut builder, "172.16.1.0/24", PrefixPool::Loopback);
            pool(&mut builder, "2001:db8::/120", PrefixPool::Loopback);
            pool(&mut builder, "172.17.1.0/24", PrefixPool::Transfer);
            Net {
                builder,
                r1,
                r2,
                e1,
                e2,
            }
        }
        fn interface(&mut self, id: InterfaceId) -> &mut Interface {
            self.builder.interfaces.get_mut(&id).unwrap()
        }
        fn findings(self) -> Vec<Finding> {
            self.builder.build().findings().to_vec()
        }
    }

    fn router(builder: &mut TopologyBuilder, name: &str, loopback: &str) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros: true,
                ..Device::default()
            },
        );
        let address = address(builder, loopback);
        let lo = builder.interfaces.next_id();
        builder.interfaces.insert(
            lo,
            Interface {
                name: "lo".into(),
                device: id,
                external: Some(PhysicalPortId::Loopback),
                port_type: Some(PortType::Loopback),
                ips: Box::new([address]),
                ..Interface::default()
            },
        );
        let device = builder.devices.get_mut(&id).unwrap();
        device.primary_ip_v4 = Some(address);
        device.loopback_ip = Some(address);
        id
    }

    fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: &str) -> InterfaceId {
        let address = address(builder, ip);
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external: PhysicalPortId::from_str(name).ok(),
                port_type: Some(PortType::Ethernet),
                ips: Box::new([address]),
                use_ospf: true,
                ..Interface::default()
            },
        );
        id
    }

    fn address(builder: &mut TopologyBuilder, ip: &str) -> crate::topology::IpAddressId {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: ip.parse().unwrap(),
                interface: None,
                prefix: None,
            },
        );
        id
    }

    fn pool(builder: &mut TopologyBuilder, prefix: &str, pool: PrefixPool) -> IpPrefixId {
        let id = builder.ip_prefixes.next_id();
        builder.ip_prefixes.insert(
            id,
            IpPrefixData {
                prefix: prefix.parse().unwrap(),
                addresses: Box::new([]),
                children: Box::new([]),
                parent: None,
                ranges: Box::new([]),
                pool: Some(pool),
                site: None,
            },
        );
        id
    }

    #[test]
    fn a_well_modeled_link_has_no_findings() {
        assert_eq!(Net::new().findings(), vec![]);
    }

    #[test]
    fn mtu_and_ospf_differing_on_a_cable_are_errors() {
        let mut net = Net::new();
        net.interface(net.e2).mtu = Some(2000);
        net.interface(net.e2).use_ospf = false;
        let (e1, e2) = (net.e1, net.e2);
        // r2 has no `ospf` left at all
        let (unannounced, findings): (Vec<_>, Vec<_>) = net
            .findings()
            .into_iter()
            .partition(|f| matches!(f.kind, FindingKind::LoopbackWithoutOspf(_)));
        assert_eq!(unannounced.len(), 1, "{unannounced:#?}");
        assert_eq!(findings.len(), 2, "{findings:#?}");
        for finding in &findings {
            assert_eq!(finding.severity, Severity::Error);
            assert_eq!(finding.devices.len(), 2);
        }
        assert!(matches!(&findings[0].kind, FindingKind::MtuMismatch(m)
            if m.mtu == 1500 && m.far_mtu == 2000 && m.interface.id == e1.0));
        assert!(matches!(&findings[1].kind, FindingKind::OspfOnOneEnd(o)
            if o.interface.id == e1.0 && o.far_end.id == e2.0));
    }

    #[test]
    fn a_cable_between_different_networks_is_an_error() {
        let mut net = Net::new();
        let e2 = net.e2;
        let other = address(&mut net.builder, "172.17.1.5/30");
        net.interface(e2).ips = Box::new([other]);
        let findings = net.findings();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(matches!(findings[0].kind, FindingKind::NoCommonSubnet(_)));
    }

    #[test]
    fn a_link_without_ospf_is_a_hint() {
        let mut net = Net::new();
        let (e1, e2) = (net.e1, net.e2);
        net.interface(e1).use_ospf = false;
        net.interface(e2).use_ospf = false;
        let (unannounced, findings): (Vec<_>, Vec<_>) = net
            .findings()
            .into_iter()
            .partition(|f| matches!(f.kind, FindingKind::LoopbackWithoutOspf(_)));
        assert_eq!(unannounced.len(), 2, "{unannounced:#?}");
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Hint);
        assert!(matches!(findings[0].kind, FindingKind::AddressWithoutOspf(_)));
    }

    #[test]
    fn a_link_to_a_device_without_routeros_is_not_checked() {
        let mut net = Net::new();
        net.builder.devices.get_mut(&net.r2).unwrap().has_routeros = false;
        let e2 = net.e2;
        net.interface(e2).use_ospf = false;
        net.interface(e2).mtu = Some(2000);
        assert_eq!(net.findings(), vec![]);
    }

    /// A radio bridge device is L2 only: its end has neither address nor `ospf`, the router's
    /// OSPF goes through it to the other side. With a loopback but no `ospf` nothing announces
    /// it (schopf-burri).
    #[test]
    fn ospf_to_a_bridge_device_without_address_is_no_finding() {
        let mut net = Net::new();
        let (r2, e2) = (net.r2, net.e2);
        net.interface(e2).use_ospf = false;
        net.interface(e2).ips = Box::new([]);
        let findings = net.findings();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(&*findings[0].devices, [r2]);
        assert!(matches!(&findings[0].kind, FindingKind::LoopbackWithoutOspf(l)
            if &*l.address.address == "172.16.1.2/32"));
        // without its loopback it is reached by DHCP
        net = Net::new();
        net.interface(e2).use_ospf = false;
        net.interface(e2).ips = Box::new([]);
        net.builder.devices.get_mut(&r2).unwrap().primary_ip_v4 = None;
        net.builder.interfaces.retain(|_, i| !(i.device == r2 && &*i.name == "lo"));
        let findings = net.findings();
        assert!(
            findings.iter().all(|f| !matches!(f.kind, FindingKind::LoopbackWithoutOspf(_))),
            "{findings:#?}"
        );
    }

    #[test]
    fn primary_ipv4_and_loopback() {
        let mut net = Net::new();
        let (r1, r2, e2) = (net.r1, net.r2, net.e2);
        net.builder.devices.get_mut(&r1).unwrap().primary_ip_v4 = None;
        let on_port = net.interface(e2).ips[0];
        net.builder.devices.get_mut(&r2).unwrap().primary_ip_v4 = Some(on_port);
        net.builder.interfaces.retain(|_, i| !(i.device == r2 && &*i.name == "lo"));
        let kinds = net
            .findings()
            .into_iter()
            .map(|f| (f.severity, f.kind))
            .collect::<Vec<_>>();
        assert_eq!(kinds.len(), 3, "{kinds:#?}");
        assert!(matches!(&kinds[0], (Severity::Error, FindingKind::NoPrimaryIpv4(n)) if &*n.device.name == "r1"));
        assert!(matches!(&kinds[1], (Severity::Warning, FindingKind::NoLoopback(n)) if &*n.device.name == "r2"));
        assert!(matches!(&kinds[2], (Severity::Warning, FindingKind::PrimaryIpv4NotOnLoopback(p))
            if p.interface.as_ref().is_some_and(|i| &*i.name == "ether1")));
    }

    /// `TopologyBuilder` makes a prefix for every address, so the test takes it away after and
    /// collects the findings again.
    #[test]
    fn an_address_without_its_prefix_is_an_error_where_dhcp_needs_it() {
        let mut net = Net::new();
        let e1 = net.e1;
        net.interface(e1).enable_dhcp_server = true;
        let mut topology = net.builder.build();
        let address = topology.interfaces[&e1].ips[0];
        topology.ip_addresses.get_mut(&address).unwrap().prefix = None;
        let findings = topology.with_findings().findings().to_vec();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(matches!(&findings[0].kind, FindingKind::AddressWithoutPrefix(a)
            if a.dhcp && &*a.address.address == "172.17.1.1/30"));
    }

/// The cabled /30 has both hosts taken; the generator makes no DHCP server there. A port with
    /// one host of its /30 left gets one, so no finding.
    #[test]
    fn dhcp_without_a_free_address_is_an_error() {
        let mut net = Net::new();
        let (r1, e1) = (net.r1, net.e1);
        net.interface(e1).enable_dhcp_server = true;
        let customer = port(&mut net.builder, r1, "ether2", "10.200.1.9/30");
        net.interface(customer).enable_dhcp_server = true;
        net.interface(customer).use_ospf = false;
        let findings = net.findings();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(matches!(&findings[0].kind, FindingKind::DhcpPoolEmpty(d)
            if &*d.address.address == "172.17.1.1/30"
                && d.prefix.as_ref().is_some_and(|p| &*p.prefix == "172.17.1.0/30")));
    }

    /// A /32 with the tag `dhcp` (router-stall-burri's loopback) has no address for a client
    /// and no prefix of its own: the generator makes no DHCP server there.
    #[test]
    fn dhcp_on_a_host_address_is_an_error() {
        let mut net = Net::new();
        let r1 = net.r1;
        let lo = port(&mut net.builder, r1, "lo", "172.16.1.60/32");
        net.interface(lo).enable_dhcp_server = true;
        net.interface(lo).use_ospf = false;
        let mut topology = net.builder.build();
        let address = topology.interfaces[&lo].ips[0];
        topology.ip_addresses.get_mut(&address).unwrap().prefix = None;
        let findings = topology.with_findings().findings().to_vec();
        assert!(
            findings.iter().any(|f| matches!(&f.kind, FindingKind::DhcpPoolEmpty(d)
                if &*d.address.address == "172.16.1.60/32" && d.prefix.is_none())),
            "{findings:#?}"
        );
    }

/// Of two WLANs, the one without VLAN is an error at the controller and the AP.
    #[test]
    fn a_wlan_without_vlan_is_an_error() {
        use crate::topology::{
            VlanData, VlanGroupData, WlanAuth, WlanData, WlanGroupData, WlanOpenSettings,
        };
        let mut net = Net::new();
        let builder = &mut net.builder;
        let (group, vlan_group) = (builder.wlan_groups.next_id(), builder.vlan_groups.next_id());
        let vlan = builder.vlans.next_id();
        let (with, without) = (builder.wlans.next_id(), builder.wlans.next_id());
        builder.vlan_groups.insert(vlan_group, VlanGroupData { vlans: Box::new([vlan]) });
        builder.vlans.insert(
            vlan,
            VlanData {
                name: "kunde".into(),
                vlan_id: 10,
                group: vlan_group,
                terminations: Box::new([]),
                vxlan: None,
                wlans: Box::new([with]),
            },
        );
        for (id, ssid, vlan) in [(with, "kunde", Some(vlan)), (without, "walter", None)] {
            builder.wlans.insert(
                id,
                WlanData {
                    ssid: ssid.into(),
                    vlan,
                    wlan_auth: WlanAuth::Open(WlanOpenSettings { use_owe: false }),
                    wlan_group: group,
                },
            );
        }
        builder.wlan_groups.insert(
            group,
            WlanGroupData {
                name: "Spitzwies".into(),
                mgmt_vlan: None,
                controller: net.r1,
                aps: Box::new([net.r2]),
                wlans: Box::new([with, without]),
            },
        );
        let (r1, r2) = (net.r1, net.r2);
        let findings = net.findings();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(&*findings[0].devices, [r1, r2]);
        assert!(matches!(&findings[0].kind, FindingKind::WlanWithoutVlan(w)
            if &*w.ssid == "walter" && &*w.group.name == "Spitzwies"));
    }

    /// Of the two ends of a wireless link, the one without RF role is reported.
    #[test]
    fn a_link_end_without_rf_role_is_a_warning() {
        use crate::topology::{Radio, RadioRole};
        let mut net = Net::new();
        for (device, radio) in [
            (net.r1, None),
            (
                net.r2,
                Some(Radio {
                    role: RadioRole::Station,
                    frequency: None,
                    channel_width: None,
                    tx_power: None,
                    link: None,
                }),
            ),
        ] {
            let id = net.builder.interfaces.next_id();
            net.builder.interfaces.insert(
                id,
                Interface {
                    name: "wlan60-1".into(),
                    device,
                    external: Some(PhysicalPortId::W60g(1)),
                    port_type: Some(PortType::Wireless),
                    radio,
                    wireless_link: Some(crate::topology::WirelessLinkEnd {
                        id: 1,
                        peer: None,
                        link: crate::topology::RadioLink {
                            ssid: "link".into(),
                            encrypted: false,
                            psk: None,
                            country: None,
                            frequency_mode: None,
                        },
                    }),
                    ..Interface::default()
                },
            );
        }
        let r1 = net.r1;
        let findings = net.findings();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(&*findings[0].devices, [r1]);
        assert!(matches!(&findings[0].kind, FindingKind::RadioWithoutRole(r)
            if &*r.interface.name == "wlan60-1"));
    }

    #[test]
    fn port_like_names_and_vlans_without_group() {
        let mut net = Net::new();
        let r1 = net.r1;
        let id = net.builder.interfaces.next_id();
        net.builder.interfaces.insert(
            id,
            Interface {
                name: "ether3-alt".into(),
                device: r1,
                port_type: Some(PortType::Ethernet),
                ..Interface::default()
            },
        );
        let mut topology = net.builder.build();
        topology.interfaces.get_mut(&id).unwrap().vlan = Some(VlanId(99));
        let findings = topology.with_findings().findings().to_vec();
        assert_eq!(findings.len(), 2, "{findings:#?}");
        assert!(matches!(findings[0].kind, FindingKind::PortLikeName(_)));
        assert!(matches!(findings[1].kind, FindingKind::VlanWithoutGroup(VlanWithoutGroup { vlan: 99, .. })));
    }

    #[test]
    fn missing_and_full_pools() {
        let mut builder = TopologyBuilder::default();
        pool(&mut builder, "172.16.1.0/29", PrefixPool::Loopback);
        for ip in 0..8 {
            address(&mut builder, &format!("172.16.1.{ip}/32"));
        }
        pool(&mut builder, "172.17.1.0/28", PrefixPool::Transfer);
        for net in ["172.17.1.0/30", "172.17.1.4/30", "172.17.1.8/29"] {
            let id = builder.ip_prefixes.next_id();
            builder.ip_prefixes.insert(
                id,
                IpPrefixData {
                    prefix: net.parse().unwrap(),
                    addresses: Box::new([]),
                    children: Box::new([]),
                    parent: None,
                    ranges: Box::new([]),
                    pool: None,
                    site: None,
                },
            );
        }
        let findings = builder.build().findings().to_vec();
        let kinds = findings.into_iter().map(|f| f.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                FindingKind::PoolNearlyFull(PoolNearlyFull {
                    pool: PrefixPool::Loopback,
                    version: IpVersion::V4,
                    free: 0,
                    size: 8,
                }),
                FindingKind::PoolMissing(PoolMissing {
                    pool: PrefixPool::Loopback,
                    version: IpVersion::V6,
                }),
                FindingKind::PoolNearlyFull(PoolNearlyFull {
                    pool: PrefixPool::Transfer,
                    version: IpVersion::V4,
                    free: 0,
                    size: 16,
                }),
            ]
        );
    }

    /// Counts what the stored production response has, by kind and severity, into
    /// `target/netbox-example/findings.txt` for review.
    #[test]
    fn findings_of_netbox_example() {
        let topology = example_topology();
        let mut counts = BTreeMap::<String, usize>::new();
        let mut lines = Vec::new();
        for finding in topology.findings() {
            let kind = format!("{:?}", finding.kind);
            let name = kind.split('(').next().unwrap_or_default().to_string();
            *counts.entry(format!("{:?} {name}", finding.severity)).or_default() += 1;
            lines.push(format!("{:?} {kind}", finding.severity));
        }
        let summary = counts
            .iter()
            .map(|(kind, count)| format!("{count:4} {kind}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/netbox-example");
        std::fs::create_dir_all(&out_dir).expect("output directory");
        std::fs::write(
            out_dir.join("findings.txt"),
            format!("{summary}\n\n{}\n", lines.join("\n")),
        )
        .expect("write findings");
        println!("{summary}");
        assert!(!counts.is_empty());
    }
}
