use super::*;
use crate::topology::{
    Cable, CablePort, CircuitTermination, Device, DeviceId, FrontPort, Interface,
    InterfaceId, PhysicalPortId, PortType, RearPort, builder::TopologyBuilder,
};
use std::{str::FromStr, sync::Arc};

fn device(builder: &mut TopologyBuilder, name: &str, role: Option<DeviceRole>) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            role,
            ..Device::default()
        },
    );
    id
}
fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str) -> InterfaceId {
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
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
fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(k, v)| (Box::from(*k), Some(Box::from(*v))))
        .collect()
}
/// r1's ports as RouterOS names them: `ether1` renamed to `e01`, `ether2` as it is.
fn interfaces() -> Vec<Row> {
    vec![
        row(&[("name", "e01"), ("default-name", "ether1")]),
        row(&[("name", "ether2"), ("default-name", "ether2")]),
        row(&[("name", "bridge")]),
    ]
}
fn seen(local: &str, identity: &str, remote: &str) -> Row {
    row(&[
        ("interface", local),
        ("identity", identity),
        ("interface-name", remote),
        ("board", "RB5009UG+S+"),
    ])
}

/// r1 ether1 -- r2 ether1, r2 has ether2 too; r3 has ether1.
#[derive(Clone)]
struct Net {
    builder: TopologyBuilder,
    r1: DeviceId,
    r1e1: InterfaceId,
    r2e1: InterfaceId,
}
fn net() -> Net {
    let mut builder = TopologyBuilder::default();
    let r1 = device(&mut builder, "r1", None);
    let r2 = device(&mut builder, "r2", None);
    let r3 = device(&mut builder, "r3", None);
    let r1e1 = port(&mut builder, r1, "ether1");
    port(&mut builder, r1, "ether2");
    let r2e1 = port(&mut builder, r2, "ether1");
    port(&mut builder, r2, "ether2");
    port(&mut builder, r3, "ether1");
    Net {
        builder,
        r1,
        r1e1,
        r2e1,
    }
}
impl Net {
    fn compare(self, neighbors: &[Row]) -> Vec<NeighborFindingKind> {
        let r1 = Arc::new(self.builder.build())
            .get_device_by_id(&self.r1)
            .unwrap();
        compare(&r1, &interfaces(), neighbors)
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    }
}

#[test]
fn the_neighbor_of_the_cable_is_no_finding() {
    let mut net = net();
    cable(&mut net.builder, net.r1e1, net.r2e1);
    assert_eq!(net.clone().compare(&[seen("e01", "r2", "e01-r1")]), vec![]);
    assert_eq!(net.compare(&[seen("e01,bridge", "r2", "bridge/ether1")]), vec![]);
}

#[test]
fn a_neighbor_without_cable_is_a_missing_cable() {
    let kinds = net().compare(&[seen("ether2", "r2", "ether2")]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::CableMissing(c)]
        if &*c.port.name == "ether2"
            && c.seen[0].interface.as_ref().is_some_and(|i| &*i.name == "ether2")),
        "{kinds:#?}");
}

#[test]
fn another_device_or_port_than_the_cable_is_an_error() {
    let mut net = net();
    cable(&mut net.builder, net.r1e1, net.r2e1);
    let kinds = net.clone().compare(&[seen("e01", "r3", "ether1")]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::CableElsewhere(_)]), "{kinds:#?}");
    let kinds = net.compare(&[seen("e01", "r2", "ether2")]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::OtherFarPort(o)]
        if &*o.seen.interface_name == "ether2"), "{kinds:#?}");
}

#[test]
fn a_cabled_router_not_seen_is_a_hint() {
    let mut net = net();
    cable(&mut net.builder, net.r1e1, net.r2e1);
    let kinds = net.compare(&[]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborNotSeen(n)]
        if &*n.far_end.device.name == "r2"), "{kinds:#?}");
}

#[test]
fn neighbors_not_in_netbox_are_a_hint() {
    let kinds = net().compare(&[seen("ether2", "customer-ap", "eth0")]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborsNotInNetbox(n)]
        if n.seen.len() == 1), "{kinds:#?}");
}

/// `port` -- front port of a panel, its rear port -- `end` of a circuit: a building's
/// side of a fiber to another one.
fn panel_to_circuit(builder: &mut TopologyBuilder, port: InterfaceId, end: CablePort) {
    let panel = builder.devices.next_id();
    builder.devices.insert(panel, Device { name: "panel".into(), ..Device::default() });
    let front = builder.front_ports.next_id();
    let rear = builder.rear_ports.next_id();
    builder.front_ports.insert(
        front,
        FrontPort { name: "1".into(), device: panel, rear_port: Some(rear), cable: None },
    );
    builder.rear_ports.insert(
        rear,
        RearPort { name: "1".into(), device: panel, front_port: Some(front), cable: None },
    );
    for (a, b) in [
        (CablePort::Interface(port), CablePort::FrontPort(front)),
        (CablePort::RearPort(rear), end),
    ] {
        let id = builder.cables.next_id();
        builder
            .cables
            .insert(id, Cable { port_a: Box::new([a]), port_b: Box::new([b]), kind: None });
    }
}

/// The cable path goes on at the other end of a circuit, as NetBox traces it: r1 ether1
/// -- panel -- circuit -- panel -- r2 ether1 is a cable from r1 ether1 to r2 ether1.
#[test]
fn a_cable_path_goes_through_a_circuit() {
    let mut net = net();
    let a = net.builder.circuit_terminations.next_id();
    let z = net.builder.circuit_terminations.next_id();
    for (end, peer) in [(a, z), (z, a)] {
        net.builder.circuit_terminations.insert(
            end,
            CircuitTermination { circuit: "fiber-1".into(), peer: Some(peer), cable: None },
        );
    }
    panel_to_circuit(&mut net.builder, net.r1e1, CablePort::CircuitTermination(a));
    panel_to_circuit(&mut net.builder, net.r2e1, CablePort::CircuitTermination(z));
    assert_eq!(net.clone().compare(&[seen("e01", "r2", "e01-r1")]), vec![]);
    let kinds = net.compare(&[]);
    assert!(matches!(&kinds[..], [NeighborFindingKind::NeighborNotSeen(n)]
        if &*n.far_end.device.name == "r2"), "{kinds:#?}");
}

/// A radio port sees the router behind the other end of its link: no cable is missing.
#[test]
fn a_radio_port_needs_no_cable() {
    let mut net = net();
    port(&mut net.builder, net.r1, "wlan60-1");
    let seen = seen("wlan60-1", "r2", "ether1");
    assert_eq!(net.compare(&[seen]), vec![]);
}

/// A radio bridge doesn't announce itself, the router on the other side shows up instead.
#[test]
fn a_cable_to_a_bridge_device_sees_through_it() {
    let mut net = net();
    let bridge = device(&mut net.builder, "r1-r2", Some(DeviceRole::WlanBridge));
    let bridge_port = port(&mut net.builder, bridge, "ether1");
    cable(&mut net.builder, net.r1e1, bridge_port);
    assert_eq!(net.compare(&[seen("e01", "r2", "ether1")]), vec![]);
}
