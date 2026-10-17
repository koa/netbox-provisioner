//! What is missing in NetBox or doesn't fit together, computed from the topology alone (the
//! Befund „NetBox in sich“ of `docs/concepts/config-helper.md`). Only the devices with RouterOS
//! are looked at, the ones the provisioner manages. Data only: the frontend words it and links
//! each object to NetBox.
use crate::{
    device::ros::DEFAULT_MTU,
    topology::{
        DeviceId, PortType, PrefixPool, SiteId, Topology,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
            ip_addresses::IpAddressAccess,
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
    PortLikeName(PortLikeName),
    VlanWithoutGroup(VlanWithoutGroup),
    MtuMismatch(MtuMismatch),
    OspfOnOneEnd(OspfOnOneEnd),
    NoCommonSubnet(NoCommonSubnet),
    AddressWithoutOspf(AddressWithoutOspf),
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
/// A physical interface (Ethernet or radio) whose name is no RouterOS port name, so the
/// generator doesn't take it for a port.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PortLikeName {
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
        let findings = net.findings();
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
        let findings = net.findings();
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
    /// OSPF goes through it to the other side.
    #[test]
    fn ospf_to_a_bridge_device_without_address_is_no_finding() {
        let mut net = Net::new();
        let e2 = net.e2;
        net.interface(e2).use_ospf = false;
        net.interface(e2).ips = Box::new([]);
        assert_eq!(net.findings(), vec![]);
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
