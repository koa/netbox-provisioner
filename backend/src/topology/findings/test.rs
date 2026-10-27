use super::*;
use crate::{
    topology::test::example_topology,
    topology::{
        Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, IpPrefixData,
        IpPrefixId, PhysicalPortId, VlanId, builder::TopologyBuilder,
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
        // as „Verbindung“ labels them
        builder.interfaces.get_mut(&e1).unwrap().label = "r2".into();
        builder.interfaces.get_mut(&e2).unwrap().label = "r1".into();
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
            dhcp_lease_time: None,
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
    builder.vlan_groups.insert(vlan_group, VlanGroupData { name: "group".into(), vlans: Box::new([vlan]) });
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
    // the VLAN of the other WLAN has no gateway: a finding of its own
    let findings = net
        .findings()
        .into_iter()
        .filter(|f| !matches!(f.kind, FindingKind::VlanWithoutGateway(_)))
        .collect::<Vec<_>>();
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
    // the link has no peer: incomplete, a finding of its own
    let findings = net
        .findings()
        .into_iter()
        .filter(|f| !matches!(f.kind, FindingKind::RadioPathIncomplete(_)))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert_eq!(&*findings[0].devices, [r1]);
    assert!(matches!(&findings[0].kind, FindingKind::RadioWithoutRole(r)
        if &*r.interface.name == "wlan60-1"));
}

/// A radio of `device` with a wireless link whose other end NetBox doesn't have.
fn lone_radio(net: &mut Net, device: DeviceId) {
    let radio = net.builder.interfaces.next_id();
    net.builder.interfaces.insert(
        radio,
        Interface {
            name: "wlan60-1".into(),
            device,
            external: Some(PhysicalPortId::W60g(1)),
            port_type: Some(PortType::Wireless),
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

/// Each radio of a link without peer lacks the link and the cable to its router.
#[test]
fn a_radio_without_peer_and_router_is_incomplete() {
    let mut net = Net::new();
    let r2 = net.r2;
    lone_radio(&mut net, r2);
    let r2 = net.r2;
    let incomplete = net
        .findings()
        .into_iter()
        .filter(|f| matches!(f.kind, FindingKind::RadioPathIncomplete(_)))
        .collect::<Vec<_>>();
    assert_eq!(incomplete.len(), 1, "{incomplete:#?}");
    assert_eq!(incomplete[0].severity, Severity::Warning);
    assert_eq!(&*incomplete[0].devices, [r2]);
    assert!(matches!(&incomplete[0].kind, FindingKind::RadioPathIncomplete(r)
        if r.missing == [RadioPathPart::RouterCable, RadioPathPart::WirelessLink]));
}

/// A radio bridge device is reached at its loopback only: without one an error.
#[test]
fn a_bridge_device_without_loopback_is_an_error() {
    let mut net = Net::new();
    let lo = net
        .builder
        .interfaces
        .iter()
        .find(|(_, i)| i.device == net.r2 && &*i.name == "lo")
        .map(|(id, _)| *id)
        .unwrap();
    net.interface(lo).name = "loopback".into();
    let r2 = net.r2;
    lone_radio(&mut net, r2);
    let no_loopback = net
        .findings()
        .into_iter()
        .filter(|f| matches!(f.kind, FindingKind::NoLoopback(_)))
        .collect::<Vec<_>>();
    assert_eq!(no_loopback.len(), 1, "{no_loopback:#?}");
    assert_eq!(no_loopback[0].severity, Severity::Error);
}

/// A label names the device at the far end, without case and punctuation; none is a hint.
#[test]
fn a_label_not_naming_the_far_end_is_a_hint() {
    let mut net = Net::new();
    let (e1, e2) = (net.e1, net.e2);
    net.interface(e1).label = "R2 core".into();
    net.interface(e2).label = "".into();
    let findings = net.findings();
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Hint);
    assert!(matches!(&findings[0].kind, FindingKind::LabelNotFarEnd(l)
        if l.interface.id == e2.0 && l.label.is_none() && &*l.far_end.device.name == "r1"));
}

/// A VLAN `vid` in a group of its own, with a VNI (`vxlan`) or without.
fn new_vlan(net: &mut Net, vid: u16, vxlan: bool) -> VlanId {
    use crate::topology::{VlanData, VlanGroupData, VxlanData};
    let builder = &mut net.builder;
    let (id, group) = (builder.vlans.next_id(), builder.vlan_groups.next_id());
    builder.vlan_groups.insert(group, VlanGroupData { name: "group".into(), vlans: Box::new([id]) });
    let vxlan = vxlan.then(|| {
        let vxlan = builder.vxlans.next_id();
        builder.vxlans.insert(
            vxlan,
            VxlanData {
                name: format!("vlan{vid}").into(),
                vni: vid.into(),
                vlans: Box::new([id]),
            },
        );
        vxlan
    });
    builder.vlans.insert(
        id,
        VlanData {
            name: format!("vlan{vid}").into(),
            vlan_id: vid,
            group,
            terminations: Box::new([]),
            vxlan,
            wlans: Box::new([]),
        },
    );
    id
}

/// A customer port `ether2` on `device` with `vlan` untagged and `ip`, if any.
fn customer_port(net: &mut Net, device: DeviceId, vlan: VlanId, ip: Option<&str>) -> InterfaceId {
    let id = port(&mut net.builder, device, "ether2", ip.unwrap_or("10.9.9.9/24"));
    let interface = net.interface(id);
    interface.use_ospf = false;
    interface.vlan = Some(vlan);
    if ip.is_none() {
        interface.ips = Box::new([]);
    }
    id
}

/// Joined by a VNI, two devices with an address in the VLAN are two gateways in one network.
#[test]
fn two_gateways_in_a_vxlan_are_an_error() {
    let mut net = Net::new();
    let vlan = new_vlan(&mut net, 10, true);
    let (r1, r2) = (net.r1, net.r2);
    customer_port(&mut net, r1, vlan, Some("10.0.10.1/24"));
    customer_port(&mut net, r2, vlan, Some("10.0.10.2/24"));
    let findings = net
        .findings()
        .into_iter()
        .filter(|f| !matches!(f.kind, FindingKind::AddressWithoutPrefix(_)))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(&*findings[0].devices, [r1, r2]);
    assert!(matches!(&findings[0].kind, FindingKind::VlanWithSeveralGateways(f)
        if f.vlan.vid == 10 && f.interfaces.len() == 2));
}

/// Without a VNI a VLAN on two devices without a cable carrying it is two networks: one gateway
/// each is right, a device without one has none.
#[test]
fn without_a_vni_each_device_is_its_own_network() {
    let mut net = Net::new();
    let vlan = new_vlan(&mut net, 10, false);
    let (r1, r2) = (net.r1, net.r2);
    customer_port(&mut net, r1, vlan, Some("10.0.10.1/24"));
    customer_port(&mut net, r2, vlan, Some("10.0.11.1/24"));
    let gateways = |findings: Vec<Finding>| {
        findings
            .into_iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FindingKind::VlanWithSeveralGateways(_) | FindingKind::VlanWithoutGateway(_)
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(gateways(net.findings()), vec![]);

    let mut net = Net::new();
    let vlan = new_vlan(&mut net, 10, false);
    let r2 = net.r2;
    customer_port(&mut net, r2, vlan, None);
    let findings = gateways(net.findings());
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert_eq!(&*findings[0].devices, [r2]);
}

/// Tagged VLANs on an Ethernet port, its own or its bridge's, are a warning.
#[test]
fn tagged_vlans_on_a_port_are_a_warning() {
    let mut net = Net::new();
    let vlan = new_vlan(&mut net, 20, true);
    let e1 = net.e1;
    net.interface(e1).tagged_vlans = Box::new([vlan]);
    let findings = net
        .findings()
        .into_iter()
        .filter(|f| matches!(f.kind, FindingKind::TaggedVlanOnPort(_)))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert!(matches!(&findings[0].kind, FindingKind::TaggedVlanOnPort(f)
        if f.interface.id == e1.0 && f.vlans.iter().map(|v| v.vid).eq([20])));
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
                dhcp_lease_time: None,
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
