//! Devices without OSPF, and their edges, stay off the map (`build`'s `supports_ospf` filter).

use super::*;
use crate::network_state::edges::Lsdb;
use crate::topology::{
    Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, IpAddressId, PhysicalPortId, PortType, Topology,
    builder::TopologyBuilder,
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

fn device(builder: &mut TopologyBuilder, name: &str, has_routeros: bool, loopback: Option<&str>) -> DeviceId {
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

fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: Option<&str>, ospf: bool) -> InterfaceId {
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
    let devices = [r1, r2].map(|id| topology.get_device_by_id(&id).unwrap()).to_vec();
    (devices, topology)
}

/// A device without RouterOS (a customer's printer) stays off the map, with its cable; the
/// cable between the two OSPF routers stays. No readers (none needed: r1 and r2 support OSPF
/// through their own `ospf` port regardless).
#[test]
fn a_device_without_routeros_is_off_the_map() {
    let (devices, _topology) = net(|builder| device(builder, "printer", false, None));
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    assert_eq!(state.nodes.iter().map(|n| &*n.device.name).collect::<Vec<_>>(), ["r1", "r2"]);
    let edges = state.edges.iter().map(|e| (&*e.a.device.name, &*e.b.device.name)).collect::<Vec<_>>();
    assert_eq!(edges, [("r1", "r2")], "the edge to the printer is left out, the one between r1 and r2 isn't");
}

/// A RouterOS device without an `ospf` port anywhere stays off the map too.
#[test]
fn a_routeros_device_without_ospf_is_off_the_map() {
    let (devices, _topology) = net(|builder| device(builder, "r3", true, Some("172.16.1.3/32")));
    let state = build(&devices, &[], &Lsdb::default(), None, 0, None);
    assert_eq!(state.nodes.iter().map(|n| &*n.device.name).collect::<Vec<_>>(), ["r1", "r2"]);
    let edges = state.edges.iter().map(|e| (&*e.a.device.name, &*e.b.device.name)).collect::<Vec<_>>();
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
