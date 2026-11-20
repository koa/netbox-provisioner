//! Scope: Tests of `helper::connect_device`; nothing else.
use super::*;
use crate::{
    helper::PlanStep,
    topology::{PrefixPool, access::ip_addresses::IpAddressAccess, test::example_topology},
};

/// A RouterOS device's port with a link in the transfer pool to another router, directly.
fn linked_port(topology: &Arc<Topology>) -> InterfaceAccess {
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let in_pool = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| pools.iter().any(|pool| pool.net.contains(&net.addr())))
    };
    topology
        .list_devices()
        .filter(|device| device.has_routeros())
        .flat_map(|device| device.interfaces().into_vec())
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
    topology
        .list_devices()
        .filter(|device| device.has_routeros() && device.id() != not.id())
        .flat_map(|device| device.interfaces().into_vec())
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

/// Another port of the device to the same router port (the Anschluss changed): the old link
/// goes, the router's port is free for the new one.
#[test]
fn the_anschluss_moves_on_the_device() {
    let topology = example_topology();
    let (device_port, other) = topology
        .list_devices()
        .filter(|device| device.has_routeros())
        .flat_map(|device| device.interfaces().into_vec())
        .filter(|port| port.cable().is_some() && !port.ips().is_empty())
        .filter(|port| {
            port.connected_interfaces()
                .first()
                .is_some_and(|far| far.device().is_some_and(|d| d.has_routeros()))
        })
        .find_map(|port| {
            let device = port.device()?;
            let other = device.interfaces().into_vec().into_iter().find(|other| {
                other.is_ethernet_port()
                    && other.cable().is_none()
                    && other.bridge().is_none()
                    && other.ips().is_empty()
            })?;
            Some((port, other))
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
