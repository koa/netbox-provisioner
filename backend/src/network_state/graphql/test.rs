//! Scope: Tests of `network_state::graphql`; nothing else.
//!
//! Devices without OSPF, and their edges, stay off the map (`build`'s `on_map_devices`), unless
//! they relay between two OSPF neighbors.

use super::*;
use crate::device::poe::{EthernetPort, PoePorts, monitor_reading};
use crate::network_state::{
    edges::Lsdb,
    lsa::{Lsa, lsa_state},
};
use crate::topology::{
    Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, IpAddressId, PhysicalPortId,
    PortType, Topology, builder::TopologyBuilder,
};
use std::{str::FromStr, sync::Arc};

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

fn device(
    builder: &mut TopologyBuilder,
    name: &str,
    has_routeros: bool,
    loopback: Option<&str>,
) -> DeviceId {
    let primary = loopback.map(|ip| address(builder, ip));
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros,
            primary_ip_v4: primary,
            loopback_ip: primary,
            ..Device::default()
        },
    );
    id
}

fn port(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    name: &str,
    ip: Option<&str>,
    ospf: bool,
) -> InterfaceId {
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
            use_ospf: ospf,
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

/// r1 and r2, both OSPF routers, cabled to each other; r2 also cabled to a third device built
/// by `far` (without RouterOS, or RouterOS without an `ospf` port).
fn net(far: impl FnOnce(&mut TopologyBuilder) -> DeviceId) -> (Vec<DeviceAccess>, Arc<Topology>) {
    let mut builder = TopologyBuilder::default();
    let r1 = device(&mut builder, "r1", true, Some("172.16.1.1/32"));
    let r2 = device(&mut builder, "r2", true, Some("172.16.1.2/32"));
    let e1 = port(&mut builder, r1, "ether1", Some("172.17.1.1/30"), true);
    let e2 = port(&mut builder, r2, "ether1", Some("172.17.1.2/30"), true);
    cable(&mut builder, e1, e2);
    let e3 = port(&mut builder, r2, "ether2", None, false);
    let far_device = far(&mut builder);
    let e4 = port(&mut builder, far_device, "ether1", None, false);
    cable(&mut builder, e3, e4);
    let topology = Arc::new(builder.build());
    let devices = [r1, r2]
        .map(|id| topology.get_device_by_id(&id).unwrap())
        .to_vec();
    (devices, topology)
}

/// A device without RouterOS (a customer's printer) stays off the map, with its cable; the
/// cable between the two OSPF routers stays. No readers (none needed: r1 and r2 support OSPF
/// through their own `ospf` port regardless).
#[test]
fn a_device_without_routeros_is_off_the_map() {
    let (devices, _topology) = net(|builder| device(builder, "printer", false, None));
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    assert_eq!(
        state
            .nodes
            .iter()
            .map(|n| &*n.device.name)
            .collect::<Vec<_>>(),
        ["r1", "r2"]
    );
    let edges = state
        .edges
        .iter()
        .map(|e| (&*e.a.device.name, &*e.b.device.name))
        .collect::<Vec<_>>();
    assert_eq!(
        edges,
        [("r1", "r2")],
        "the edge to the printer is left out, the one between r1 and r2 isn't"
    );
}

/// A RouterOS device without an `ospf` port anywhere stays off the map too.
#[test]
fn a_routeros_device_without_ospf_is_off_the_map() {
    let (devices, _topology) = net(|builder| device(builder, "r3", true, Some("172.16.1.3/32")));
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    assert_eq!(
        state
            .nodes
            .iter()
            .map(|n| &*n.device.name)
            .collect::<Vec<_>>(),
        ["r1", "r2"]
    );
    let edges = state
        .edges
        .iter()
        .map(|e| (&*e.a.device.name, &*e.b.device.name))
        .collect::<Vec<_>>();
    assert_eq!(edges, [("r1", "r2")]);
}

/// Both OSPF routers stay on the map, with the cable between them, and neither is reachable
/// (no LSDB): `Missing`.
#[test]
fn both_ospf_routers_are_on_the_map() {
    let (devices, _topology) = net(|builder| device(builder, "printer", false, None));
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    assert_eq!(state.nodes.len(), 2);
    assert!(state.nodes.iter().all(|n| n.state == NodeState::Missing));
    assert_eq!(state.edges.len(), 1);
}

/// r1 and r2, both OSPF routers with an address in the same /30, cabled only through a plain
/// switch without RouterOS: the segment an L2 run carries over several cables
/// (`docs/network-map.md`).
fn bridged_net() -> (Vec<DeviceAccess>, Arc<Topology>) {
    let mut builder = TopologyBuilder::default();
    let r1 = device(&mut builder, "r1", true, Some("172.16.1.1/32"));
    let r2 = device(&mut builder, "r2", true, Some("172.16.1.2/32"));
    let sw = device(&mut builder, "sw", false, None);
    let e1 = port(&mut builder, r1, "ether1", Some("172.17.1.1/30"), true);
    let e2 = port(&mut builder, r2, "ether1", Some("172.17.1.2/30"), true);
    let s1 = port(&mut builder, sw, "ether1", None, false);
    let s2 = port(&mut builder, sw, "ether2", None, false);
    cable(&mut builder, e1, s1);
    cable(&mut builder, s2, e2);
    let topology = Arc::new(builder.build());
    let devices = [r1, r2]
        .map(|id| topology.get_device_by_id(&id).unwrap())
        .to_vec();
    (devices, topology)
}

/// r1 and r2, cabled on `ether1`: r1's port is `pse` in NetBox, r2's `pd`.
fn poe_net() -> (Vec<DeviceAccess>, Arc<Topology>) {
    let mut builder = TopologyBuilder::default();
    let r1 = device(&mut builder, "r1", true, Some("172.16.1.1/32"));
    let r2 = device(&mut builder, "r2", true, Some("172.16.1.2/32"));
    let e1 = port(&mut builder, r1, "ether1", Some("172.17.1.1/30"), true);
    let e2 = port(&mut builder, r2, "ether1", Some("172.17.1.2/30"), true);
    builder.interfaces.get_mut(&e1).unwrap().poe = Some(PoeMode::Pse);
    builder.interfaces.get_mut(&e2).unwrap().poe = Some(PoeMode::Pd);
    cable(&mut builder, e1, e2);
    let topology = Arc::new(builder.build());
    let devices = [r1, r2]
        .map(|id| topology.get_device_by_id(&id).unwrap())
        .to_vec();
    (devices, topology)
}

/// r1 feeds r2 over their cable, `pse`/`pd` as NetBox has it, reporting `powered-on` with 3.9 W:
/// one PSE→PD arrow (`docs/network-map.md`, step 4).
#[test]
fn a_feeding_port_gets_a_poe_edge() {
    let (devices, _topology) = poe_net();
    let data = Arc::new(DeviceMapData {
        poe: PoePorts {
            ethernet: vec![EthernetPort {
                name: "ether1".into(),
                default_name: "ether1".into(),
                poe_out: true,
            }]
            .into(),
            monitor: vec![monitor_reading(&[
                ("name", "ether1"),
                ("poe-out-status", "powered-on"),
                ("poe-out-power", "3.9"),
            ])]
            .into(),
        },
        ..DeviceMapData::default()
    });
    let device_data = BTreeMap::from([(devices[0].id(), data)]);
    let details = build_details(&devices, &device_data);
    assert_eq!(
        details.poe_edges.len(),
        1,
        "{:?}",
        details
            .poe_edges
            .iter()
            .map(|e| &e.pse.name)
            .collect::<Vec<_>>()
    );
    let edge = &details.poe_edges[0];
    assert_eq!(&*edge.pse.device.name, "r1");
    assert_eq!(&*edge.pd.device.name, "r2");
    assert_eq!(edge.power, Some(3.9));
    assert_eq!(edge.problem, None);
}

/// Without a device reading for r2 (it didn't answer, or isn't polled), r1's PoE reading alone
/// is still enough: the far end comes from NetBox's cable, not from r2's own reading.
#[test]
fn a_poe_edge_needs_no_reading_from_the_far_device() {
    let (devices, _topology) = poe_net();
    let data = Arc::new(DeviceMapData {
        poe: PoePorts {
            ethernet: vec![EthernetPort {
                name: "ether1".into(),
                default_name: "ether1".into(),
                poe_out: true,
            }]
            .into(),
            monitor: vec![monitor_reading(&[
                ("name", "ether1"),
                ("poe-out-status", "waiting-for-load"),
            ])]
            .into(),
        },
        ..DeviceMapData::default()
    });
    let device_data = BTreeMap::from([(devices[0].id(), data)]);
    let details = build_details(&devices, &device_data);
    assert_eq!(
        details.poe_edges.len(),
        1,
        "pse in NetBox draws the arrow even while waiting for load"
    );
    assert_eq!(details.poe_edges[0].problem, Some(PoeProblem::NoLoad));
}

fn row(fields: &[(&str, &str)]) -> Option<Lsa> {
    Lsa::from_state(&lsa_state(fields))
}

fn router(originator: &'static str, body: &'static str) -> [(&'static str, &'static str); 5] {
    [
        ("instance", "default-v2"),
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
    (
        "body",
        "netmask=255.255.255.252\n    router-id=172.16.1.1\n    router-id=172.16.1.2\n",
    ),
];

/// The switch between r1 and r2 isn't itself an OSPF router, but the LSDB confirms r1 and r2
/// are neighbors over the segment it carries: it, and both its cables, stay on the map, and r1
/// and r2 are `Reached` even though no single cable of theirs is ever `EdgeState::Up` (a plain
/// device's port is never an OSPF end).
#[test]
fn a_switch_between_ospf_neighbors_is_on_the_map() {
    let (devices, _topology) = bridged_net();
    let lsdb = [
        router("172.16.1.1", R1_V2),
        router("172.16.1.2", R2_V2),
        SEGMENT_V2,
    ]
    .iter()
    .filter_map(|fields| row(fields))
    .collect::<Lsdb>();
    let state = build(&devices, &[], &lsdb, None, 0, None);
    let mut names = state
        .nodes
        .iter()
        .map(|n| &*n.device.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["r1", "r2", "sw"]);
    let state_of = |name: &str| {
        state
            .nodes
            .iter()
            .find(|n| &*n.device.name == name)
            .unwrap()
            .state
    };
    assert_eq!(
        state_of("sw"),
        NodeState::NotOspf,
        "the switch doesn't run OSPF itself"
    );
    assert_eq!(state_of("r1"), NodeState::Reached);
    assert_eq!(state_of("r2"), NodeState::Reached);
    assert_eq!(
        state.edges.len(),
        2,
        "both cables through the switch are shown"
    );
}

/// Without the LSDB confirming the two routers share a segment, the switch stays off: it might
/// be relaying something else entirely, not necessarily this link.
#[test]
fn a_switch_without_a_confirmed_segment_is_off_the_map() {
    let (devices, _topology) = bridged_net();
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    let names = state
        .nodes
        .iter()
        .map(|n| &*n.device.name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["r1", "r2"],
        "sw stays off without proof it relays a real OSPF neighbor"
    );
    assert!(
        state.edges.is_empty(),
        "neither cable through sw is shown without that proof"
    );
}
