use super::*;
use crate::network_state::lsa::Lsa;
use crate::topology::{
    Cable, CablePort, Device, Interface, IpAddressData, IpAddressId, PhysicalPortId, PortType, Topology,
    builder::TopologyBuilder,
};
use std::{str::FromStr, sync::Arc};

/// r1 (router ID 172.16.1.1) and r2 (172.16.1.2), `ether1` cabled to each other in
/// 172.17.1.0/30 with `ospf`; r2's `ether2` to a device without RouterOS.
struct Net {
    topology: Arc<Topology>,
    r1: DeviceId,
    r2: DeviceId,
}

fn address(builder: &mut TopologyBuilder, ip: &str) -> IpAddressId {
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

fn device(builder: &mut TopologyBuilder, name: &str, loopback: Option<&str>) -> DeviceId {
    let primary = loopback.map(|ip| address(builder, ip));
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: primary.is_some(),
            primary_ip_v4: primary,
            loopback_ip: primary,
            ..Device::default()
        },
    );
    id
}

fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: Option<&str>) -> InterfaceId {
    let ips = ip.map(|ip| address(builder, ip)).into_iter().collect();
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            ips,
            use_ospf: ip.is_some(),
            ..Interface::default()
        },
    );
    id
}

fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
    let id = builder.cables.next_id();
    builder.cables.insert(
        id,
        Cable {
            port_a: Box::new([CablePort::Interface(a)]),
            port_b: Box::new([CablePort::Interface(b)]),
            kind: None,
        },
    );
}

impl Net {
    fn new() -> Self {
        let mut builder = TopologyBuilder::default();
        let r1 = device(&mut builder, "r1", Some("172.16.1.1/32"));
        let r2 = device(&mut builder, "r2", Some("172.16.1.2/32"));
        let customer = device(&mut builder, "printer", None);
        let e1 = port(&mut builder, r1, "ether1", Some("172.17.1.1/30"));
        let e2 = port(&mut builder, r2, "ether1", Some("172.17.1.2/30"));
        cable(&mut builder, e1, e2);
        let e3 = port(&mut builder, r2, "ether2", None);
        let e4 = port(&mut builder, customer, "ether1", None);
        cable(&mut builder, e3, e4);
        Net {
            topology: Arc::new(builder.build()),
            r1,
            r2,
        }
    }
    /// The states of the edges, the OSPF one first.
    fn states(&self, lsas: &[&[(&str, &str)]]) -> Vec<EdgeState> {
        let lsdb = lsas.iter().filter_map(|fields| row(fields)).collect::<Lsdb>();
        let devices = [self.r1, self.r2].map(|id| self.topology.get_device_by_id(&id).unwrap());
        let mut edges = edges(&devices, &lsdb);
        edges.sort_by_key(|edge| edge.a);
        edges.into_iter().map(|edge| edge.state).collect()
    }
}

fn row(fields: &[(&str, &str)]) -> Option<Lsa> {
    Lsa::from_row(|key| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
}

fn router(version: &'static str, originator: &'static str, body: &'static str) -> [(&'static str, &'static str); 5] {
    [
        ("instance", version),
        ("type", "router"),
        ("originator", originator),
        ("id", originator),
        ("body", body),
    ]
}

const R1_V2: &str = "options=E\n    type=network id=172.17.1.1 data=172.17.1.1 metric=100\n";
const R2_V2: &str = "options=E\n    type=network id=172.17.1.1 data=172.17.1.2 metric=100\n";
const SEGMENT_V2: [(&str, &str); 5] = [
    ("instance", "default-v2"),
    ("type", "network"),
    ("originator", "172.16.1.1"),
    ("id", "172.17.1.1"),
    ("body", "netmask=255.255.255.252\n    router-id=172.16.1.1\n    router-id=172.16.1.2\n"),
];
/// both name the DR r1 on the segment (the DR itself too, as on lab-np2)
const R_V3: &str = "options=V6|E|R\n    type=network iface-id=7 neighbor-iface-if=7 neighbor-router-id=172.16.1.1 metric=100\n";
const SEGMENT_V3: [(&str, &str); 5] = [
    ("instance", "default-v3"),
    ("type", "network"),
    ("originator", "172.16.1.1"),
    ("id", "0.0.0.7"),
    ("body", "options=V6|E|R\n    router-id=172.16.1.1\n    router-id=172.16.1.2\n"),
];

#[test]
fn both_routers_on_the_segment_in_v2_and_v3_is_up() {
    let net = Net::new();
    let states = net.states(&[
        &router("default-v2", "172.16.1.1", R1_V2),
        &router("default-v2", "172.16.1.2", R2_V2),
        &SEGMENT_V2,
        &router("default-v3", "172.16.1.1", R_V3),
        &router("default-v3", "172.16.1.2", R_V3),
        &SEGMENT_V3,
    ]);
    // the customer's cable says nothing in OSPF
    assert_eq!(states, [EdgeState::Up, EdgeState::NotOspf]);
}

#[test]
fn up_in_v2_without_v3_neighbors_is_v3_down() {
    let net = Net::new();
    let states = net.states(&[
        &router("default-v2", "172.16.1.1", R1_V2),
        &router("default-v2", "172.16.1.2", R2_V2),
        &SEGMENT_V2,
    ]);
    assert_eq!(states[0], EdgeState::V3Down);
}

/// The far end went away: r1 has the network as a stub, no segment is left.
#[test]
fn a_stub_without_segment_is_down() {
    let net = Net::new();
    let states = net.states(&[
        &router(
            "default-v2",
            "172.16.1.1",
            "options=E\n    type=stub id=172.17.1.0 data=255.255.255.252 metric=100\n",
        ),
        &router("default-v2", "172.16.1.2", "options=E\n"),
    ]);
    assert_eq!(states[0], EdgeState::Down(DownReason::NoSegment));
}

/// r2 went away after it was DR: its network LSA lists both until it ages out, but r1 links
/// to it no more.
#[test]
fn a_segment_left_behind_doesnt_count() {
    let net = Net::new();
    let states = net.states(&[
        &router(
            "default-v2",
            "172.16.1.1",
            "options=E\n    type=stub id=172.17.1.0 data=255.255.255.252 metric=100\n",
        ),
        &router("default-v2", "172.16.1.2", R2_V2),
        &[
            ("instance", "default-v2"),
            ("type", "network"),
            ("originator", "172.16.1.2"),
            ("id", "172.17.1.2"),
            ("body", "netmask=255.255.255.252\n    router-id=172.16.1.1\n    router-id=172.16.1.2\n"),
        ],
    ]);
    assert_eq!(states[0], EdgeState::Down(DownReason::NoSegment));
}

/// IPv6 off on r2: the v3 segment of an earlier time is left, r2 names no neighbor.
#[test]
fn a_v3_segment_left_behind_doesnt_count() {
    let net = Net::new();
    let states = net.states(&[
        &router("default-v2", "172.16.1.1", R1_V2),
        &router("default-v2", "172.16.1.2", R2_V2),
        &SEGMENT_V2,
        &router("default-v3", "172.16.1.1", R_V3),
        &router("default-v3", "172.16.1.2", "options=V6|E|R\n"),
        &SEGMENT_V3,
    ]);
    assert_eq!(states[0], EdgeState::V3Down);
}

#[test]
fn a_router_gone_from_the_area_is_named() {
    let net = Net::new();
    let states = net.states(&[&router("default-v2", "172.16.1.1", R1_V2)]);
    assert_eq!(states[0], EdgeState::Down(DownReason::RouterMissing(net.r2)));
}

/// r2 reaches the segment on another cable (a L2 segment over a switch), not this one.
#[test]
fn a_segment_without_one_of_the_routers_is_down() {
    let net = Net::new();
    let states = net.states(&[
        &router("default-v2", "172.16.1.1", R1_V2),
        &router("default-v2", "172.16.1.2", "options=E\n"),
        &[
            ("instance", "default-v2"),
            ("type", "network"),
            ("originator", "172.16.1.1"),
            ("id", "172.17.1.1"),
            ("body", "netmask=255.255.255.252\n    router-id=172.16.1.1\n    router-id=172.16.1.9\n"),
        ],
    ]);
    assert_eq!(states[0], EdgeState::Down(DownReason::NotOnSegment(net.r2)));
}
