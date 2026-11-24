//! Scope: Tests of `helper::connect_device`; nothing else.
use super::*;
use crate::{
    helper::PlanStep,
    topology::{PrefixPool, access::ip_addresses::IpAddressAccess, test::example_topology},
};

/// The devices by id, their ports by id: the topology's come in no order, a test picks the
/// same each run.
fn by_id(topology: &Arc<Topology>) -> impl Iterator<Item = DeviceAccess> {
    let mut devices = topology.list_devices().collect::<Vec<_>>();
    devices.sort_by_key(|device| device.id().0);
    devices.into_iter()
}

fn ports_by_id(device: &DeviceAccess) -> Vec<InterfaceAccess> {
    let mut ports = device.interfaces().into_vec();
    ports.sort_by_key(|port| port.id().0);
    ports
}

/// A RouterOS device's port with a link in the transfer pool to another router, directly.
fn linked_port(topology: &Arc<Topology>) -> InterfaceAccess {
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let in_pool = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| pools.iter().any(|pool| pool.net.contains(&net.addr())))
    };
    by_id(topology)
        .filter(|device| device.has_routeros())
        .flat_map(|device| ports_by_id(&device))
        .find(|port| {
            port.ips().iter().any(in_pool)
                && port.cable().is_some()
                && port.connected_interfaces().first().is_some_and(|far| {
                    far.device().is_some_and(|d| d.has_routeros()) && !far.ips().is_empty()
                })
        })
        .expect("a linked port")
}

/// A free Ethernet port of a RouterOS device other than `not`: no cable, bridge or address.
fn free_port(topology: &Arc<Topology>, not: &DeviceAccess) -> InterfaceAccess {
    by_id(topology)
        .filter(|device| device.has_routeros() && device.id() != not.id())
        .flat_map(|device| ports_by_id(&device))
        .find(|port| {
            port.is_ethernet_port()
                && port.cable().is_none()
                && port.bridge().is_none()
                && port.ips().is_empty()
        })
        .expect("a free port")
}

/// Moving a device's linked port to a free port of another router clears its old link first:
/// cable and transfer addresses go, and on what is left the port has neither.
#[test]
fn the_old_link_goes_first() {
    let topology = example_topology();
    let device_port = linked_port(&topology);
    let device = device_port.device().expect("its device");
    let port = free_port(&topology, &device);
    let input = ConnectDeviceInput {
        port: port.id().0,
        socket: None,
        device_port: device_port.id().0,
    };
    let plan = prepare(&topology, &input).expect("a plan");
    assert!(
        plan.steps
            .iter()
            .any(|step| matches!(step, PlanStep::DeleteCable(cable) if cable.port.id == device_port.id().0)),
        "{:?}",
        plan.steps
    );
    let after = plan.preview_topology().expect("a preview");
    let moved = after
        .get_interface_by_id(device_port.id())
        .expect("the port after");
    assert!(moved.cable().is_none());
    assert!(moved.ips().is_empty(), "{:?}", moved.ips());
}

/// A router's port with a VLAN loses it: a link has addresses, no VLAN.
#[test]
fn the_routers_port_loses_its_vlan() {
    let mut raw = crate::topology::test::example_response();
    let topology = crate::topology::test::topology_from(raw.clone());
    let device_port = linked_port(&topology);
    let device = device_port.device().expect("its device");
    let port = free_port(&topology, &device);
    let vlan = topology.list_vlans().next().expect("a VLAN");
    for found in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .flat_map(|device| device["interfaces"].as_array_mut().into_iter().flatten())
        .filter(|interface| {
            interface["id"].as_str().and_then(|id| id.parse().ok()) == Some(port.id().0)
        })
    {
        found["untagged_vlan"] = serde_json::json!({ "id": vlan.id.0.to_string() });
    }
    let topology = crate::topology::test::topology_from(raw);
    let input = ConnectDeviceInput {
        port: port.id().0,
        socket: None,
        device_port: device_port.id().0,
    };
    let plan = prepare(&topology, &input).expect("a plan");
    assert!(
        plan.steps.iter().any(|step| matches!(
            step,
            PlanStep::UpdatePortVlan(update) if update.interface.id == port.id().0 && update.vlan.is_none()
        )),
        "{:?}",
        plan.steps
    );
}

struct PortInBridge {
    port: InterfaceAccess,
    bridge: InterfaceAccess,
}

/// A router's port in a bridge leaves it, its VLANs with it: a link has the port for itself.
/// The bridge and its other ports stay.
#[test]
fn the_routers_port_leaves_its_bridge() {
    let mut raw = crate::topology::test::example_response();
    let topology = crate::topology::test::topology_from(raw.clone());
    let device_port = linked_port(&topology);
    let device = device_port.device().expect("its device");
    // a free port of a router with a bridge
    let PortInBridge { port, bridge } = by_id(&topology)
        .filter(|router| router.has_routeros() && router.id() != device.id())
        .find_map(|router| {
            let ports = ports_by_id(&router);
            let bridge = ports.iter().find(|other| other.is_bridge())?.clone();
            let port = ports.into_iter().find(|port| {
                port.is_ethernet_port()
                    && port.cable().is_none()
                    && port.bridge().is_none()
                    && port.ips().is_empty()
            })?;
            Some(PortInBridge { port, bridge })
        })
        .expect("a free port of a router with a bridge");
    for found in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .flat_map(|device| device["interfaces"].as_array_mut().into_iter().flatten())
        .filter(|interface| {
            interface["id"].as_str().and_then(|id| id.parse().ok()) == Some(port.id().0)
        })
    {
        found["bridge"] = serde_json::json!({ "id": bridge.id().0.to_string() });
    }
    let topology = crate::topology::test::topology_from(raw);
    let in_bridge = topology.get_interface_by_id(port.id()).expect("the port");
    assert_eq!(in_bridge.bridge().map(|b| b.id()), Some(bridge.id()));
    let input = ConnectDeviceInput {
        port: port.id().0,
        socket: None,
        device_port: device_port.id().0,
    };
    let plan = prepare(&topology, &input).expect("a plan, not PortInBridge");
    assert!(
        plan.steps.iter().any(|step| matches!(
            step,
            PlanStep::UpdatePort(update) if update.interface.id == port.id().0
                && update.bridge_before == Some(bridge.id().0)
                && update.bridge_id.is_none()
        )),
        "{:?}",
        plan.steps
    );
    let after = plan.preview_topology().expect("a preview");
    let left = after
        .get_interface_by_id(port.id())
        .expect("the port after");
    assert!(left.bridge().is_none());
    assert!(after.get_interface_by_id(bridge.id()).is_some());
}

/// Another port of the device to the same router port (the Anschluss changed): the old link
/// goes, the router's port is free for the new one.
#[test]
fn the_anschluss_moves_on_the_device() {
    let topology = example_topology();
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let in_pool = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| pools.iter().any(|pool| pool.net.contains(&net.addr())))
    };
    // a device's Anschluss with a link of the transfer pool to a router, and a free port of it;
    // the first by id, the topology's devices come in no order
    let mut devices = topology
        .list_devices()
        .filter(|device| device.has_routeros())
        .collect::<Vec<_>>();
    devices.sort_by_key(|device| device.id().0);
    let (device_port, other) = devices
        .iter()
        .find_map(|device| {
            let port = uplink_port(device)?;
            let linked = port.cable().is_some()
                && port.ips().iter().any(in_pool)
                && port
                    .connected_interfaces()
                    .first()
                    .is_some_and(|far| far.device().is_some_and(|d| d.has_routeros()));
            if !linked {
                return None;
            }
            let mut free = device
                .interfaces()
                .into_vec()
                .into_iter()
                .filter(|other| {
                    other.is_ethernet_port()
                        && other.cable().is_none()
                        && other.bridge().is_none()
                        && other.ips().is_empty()
                })
                .collect::<Vec<_>>();
            free.sort_by_key(|other| other.id().0);
            Some((port, free.into_iter().next()?))
        })
        .expect("a linked device with a free port");
    let router_port = device_port.connected_interfaces()[0].clone();
    let input = ConnectDeviceInput {
        port: router_port.id().0,
        socket: None,
        device_port: other.id().0,
    };
    let plan = prepare(&topology, &input).expect("a plan");
    let after = plan.preview_topology().expect("a preview");
    let old = after
        .get_interface_by_id(device_port.id())
        .expect("the old port");
    assert!(
        old.cable().is_none() && old.ips().is_empty(),
        "{:?}",
        plan.steps
    );
    let router_port = after
        .get_interface_by_id(router_port.id())
        .expect("the router's port");
    assert!(router_port.cable().is_none(), "{:?}", plan.steps);
}
