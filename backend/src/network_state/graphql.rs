//! `Query.topology.networkState`: a site's, or every site's, devices and edges with their state
//! from the LSDB, for the network map (`docs/concepts/network-map.md`, step 2).

use super::{
    edges::{DownReason, EdgeKind, EdgeState, Lsdb, edges},
    read::{read, readers},
};
use crate::topology::{
    DeviceId,
    access::device::DeviceAccess,
    findings::{DeviceRef, InterfaceRef, device_ref, interface_ref},
};
use async_graphql::{Enum, SimpleObject};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr},
};

#[derive(SimpleObject)]
pub struct NetworkState {
    /// the router the LSDB was read from; none if none answered (`error`) or the site has no
    /// OSPF router
    read_from: Option<DeviceRef>,
    /// seconds since the reading
    age: u32,
    /// why no router answered
    error: Option<String>,
    nodes: Vec<MapNode>,
    edges: Vec<MapEdge>,
    /// routers in the LSDB that are no device in NetBox (production behind the lab's uplink)
    outside: u32,
    /// the site's routers that share a segment with those
    outside_at: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum NodeState {
    /// the reader, or a router with an edge up
    Reached,
    /// in the LSDB, but no edge of it is up: cut off, its LSAs not yet aged out
    Isolated,
    /// an OSPF router of NetBox without router LSA
    Missing,
    /// a plain switch relaying an OSPF segment between two routers that are its neighbors
    /// there (`on_map_devices`): on the map, but no OSPF reachability of its own to judge
    NotOspf,
}

#[derive(SimpleObject)]
pub struct MapNode {
    device: DeviceRef,
    routeros: bool,
    state: NodeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum MapEdgeKind {
    Cable,
    Radio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum MapEdgeState {
    Up,
    V3Down,
    Down,
    NotOspf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum MapDownReason {
    RouterMissing,
    NoSegment,
    NotOnSegment,
}

#[derive(SimpleObject)]
pub struct MapEdge {
    kind: MapEdgeKind,
    a: InterfaceRef,
    b: InterfaceRef,
    state: MapEdgeState,
    reason: Option<MapDownReason>,
    /// the device `reason` is about
    reason_device: Option<u32>,
}

fn router_id(device: &DeviceAccess) -> Option<Ipv4Addr> {
    match device.primary_ip_v4().and_then(|ip| ip.addr()) {
        Some(IpAddr::V4(id)) => Some(id),
        _ => None,
    }
}

/// Whether `device` takes part in OSPF itself (it is the LSDB's reader, or it has RouterOS, a
/// primary IPv4 and an `ospf` interface). A device that doesn't is off the map entirely unless
/// it relays between two that do (`on_map_devices`).
fn supports_ospf(device: &DeviceAccess, readers: &[DeviceAccess]) -> bool {
    readers.iter().any(|r| r.id() == device.id())
        || (device.has_routeros() && router_id(device).is_some() && device.interfaces().iter().any(|i| i.use_ospf()))
}

/// The devices to show: those with OSPF themselves (`supports_ospf`), and every plain device
/// (a switch, an unmanaged device) on a run of cables between two that are OSPF neighbors over
/// the segment it carries (an L2 segment over several cables, `docs/concepts/network-map.md`) —
/// as opposed to a leaf with nothing OSPF beyond it (a customer's printer), which stays off.
/// Also which of the OSPF devices such a run confirms as currently adjacent, hence reached,
/// since none of its own cables can ever be `EdgeState::Up` (a plain device's port is never an
/// OSPF end).
fn on_map_devices(
    by_id: &BTreeMap<DeviceId, DeviceAccess>,
    readers: &[DeviceAccess],
    lsdb: &Lsdb,
    adjacency: &BTreeMap<DeviceId, BTreeSet<DeviceId>>,
) -> (BTreeSet<DeviceId>, BTreeSet<DeviceId>) {
    let is_ospf = |id: DeviceId| by_id.get(&id).is_some_and(|d| supports_ospf(d, readers));
    let mut on_map = by_id
        .values()
        .filter(|d| supports_ospf(d, readers))
        .map(DeviceAccess::id)
        .collect::<BTreeSet<_>>();
    let mut reached = BTreeSet::new();

    let mut seen = BTreeSet::new();
    for &start in by_id.keys() {
        if is_ospf(start) || !seen.insert(start) {
            continue;
        }
        let mut component = BTreeSet::from([start]);
        let mut boundary = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            for &neighbor in adjacency.get(&id).into_iter().flatten() {
                if is_ospf(neighbor) {
                    boundary.insert(neighbor);
                } else if component.insert(neighbor) {
                    seen.insert(neighbor);
                    stack.push(neighbor);
                }
            }
        }
        let boundary_routers = boundary.iter().filter_map(|id| by_id.get(id)).filter_map(router_id).collect::<Vec<_>>();
        let neighboring = boundary_routers.iter().enumerate().any(|(i, &router)| {
            boundary_routers[i + 1..]
                .iter()
                .any(|&other| lsdb.segment_neighbors(router).contains(&other))
        });
        if neighboring {
            on_map.extend(&component);
            reached.extend(boundary);
        }
    }
    (on_map, reached)
}

/// The state of `devices` (a site's, or every device, with RouterOS or not) as the LSDB of one
/// of them says. Devices without OSPF, and their edges, are left out.
pub async fn network_state(devices: Vec<DeviceAccess>) -> NetworkState {
    let readers = readers(&devices);
    let (read_from, age, lsdb, error) = match read(&readers).await {
        Ok(Some((device, fact))) => {
            let age = fact.age().as_secs() as u32;
            (Some(device), age, fact.value.ok(), None)
        }
        Ok(None) => (None, 0, None, None),
        Err(error) => (None, 0, None, Some(error.to_string())),
    };
    let lsdb = lsdb.unwrap_or_default();
    build(&devices, &readers, &lsdb, read_from, age, error)
}

/// The synchronous part of [`network_state`], once the LSDB (or none, read-only, after a
/// failure) is at hand: testable without a device to read from.
fn build(
    devices: &[DeviceAccess],
    readers: &[DeviceAccess],
    lsdb: &Lsdb,
    read_from: Option<DeviceAccess>,
    age: u32,
    error: Option<String>,
) -> NetworkState {
    let map_edges = edges(devices, lsdb);
    let topology = devices.first().map(|d| d.topology().clone());
    let interface = |id| topology.as_ref().and_then(|t| t.get_interface_by_id(id));

    // every device at an edge, also those of another site, and how they cable to each other
    let mut by_id = devices.iter().map(|d| (d.id(), d.clone())).collect::<BTreeMap<_, _>>();
    let mut adjacency = BTreeMap::<DeviceId, BTreeSet<DeviceId>>::new();
    for edge in &map_edges {
        let (Some(a), Some(b)) = (interface(edge.a), interface(edge.b)) else { continue };
        let (Some(device_a), Some(device_b)) = (a.device(), b.device()) else { continue };
        by_id.entry(device_a.id()).or_insert_with(|| device_a.clone());
        by_id.entry(device_b.id()).or_insert_with(|| device_b.clone());
        adjacency.entry(device_a.id()).or_default().insert(device_b.id());
        adjacency.entry(device_b.id()).or_default().insert(device_a.id());
    }
    let (on_map, mut up) = on_map_devices(&by_id, readers, lsdb, &adjacency);

    let mut nodes = BTreeMap::<DeviceId, DeviceAccess>::new();
    for device in by_id.values() {
        if on_map.contains(&device.id()) {
            nodes.insert(device.id(), device.clone());
        }
    }
    let mut result_edges = Vec::new();
    for edge in &map_edges {
        let (Some(a), Some(b)) = (interface(edge.a), interface(edge.b)) else { continue };
        let (Some(device_a), Some(device_b)) = (a.device(), b.device()) else { continue };
        if !on_map.contains(&device_a.id()) || !on_map.contains(&device_b.id()) {
            continue;
        }
        for device in [device_a, device_b] {
            if matches!(edge.state, EdgeState::Up | EdgeState::V3Down) {
                up.insert(device.id());
            }
            nodes.entry(device.id()).or_insert(device);
        }
        let (state, reason, reason_device) = match edge.state {
            EdgeState::Up => (MapEdgeState::Up, None, None),
            EdgeState::V3Down => (MapEdgeState::V3Down, None, None),
            EdgeState::NotOspf => (MapEdgeState::NotOspf, None, None),
            EdgeState::Down(DownReason::RouterMissing(d)) => {
                (MapEdgeState::Down, Some(MapDownReason::RouterMissing), Some(d.0))
            }
            EdgeState::Down(DownReason::NoSegment) => (MapEdgeState::Down, Some(MapDownReason::NoSegment), None),
            EdgeState::Down(DownReason::NotOnSegment(d)) => {
                (MapEdgeState::Down, Some(MapDownReason::NotOnSegment), Some(d.0))
            }
        };
        result_edges.push(MapEdge {
            kind: match edge.kind {
                EdgeKind::Cable => MapEdgeKind::Cable,
                EdgeKind::Radio => MapEdgeKind::Radio,
            },
            a: interface_ref(&a),
            b: interface_ref(&b),
            state,
            reason,
            reason_device,
        });
    }
    let reader = read_from.as_ref().map(DeviceAccess::id);
    let mut result_nodes = nodes
        .values()
        .map(|device| {
            let state = if !supports_ospf(device, readers) {
                NodeState::NotOspf
            } else {
                match router_id(device) {
                    _ if Some(device.id()) == reader || up.contains(&device.id()) => NodeState::Reached,
                    Some(id) if lsdb.routers().contains(&id) => NodeState::Isolated,
                    _ => NodeState::Missing,
                }
            };
            MapNode {
                device: device_ref(device),
                routeros: device.has_routeros(),
                state,
            }
        })
        .collect::<Vec<_>>();
    result_nodes.sort_by(|a, b| a.device.name.cmp(&b.device.name));

    // the routers NetBox doesn't know, and where they come in
    let known = topology
        .iter()
        .flat_map(|t| t.list_devices())
        .filter_map(|device| router_id(&device))
        .collect::<BTreeSet<_>>();
    let outside = lsdb.routers().difference(&known).copied().collect::<BTreeSet<_>>();
    let outside_at = devices
        .iter()
        .filter(|device| {
            router_id(device)
                .is_some_and(|id| lsdb.segment_neighbors(id).iter().any(|n| outside.contains(n)))
        })
        .map(|device| device.id().0)
        .collect();
    NetworkState {
        read_from: read_from.as_ref().map(device_ref),
        age,
        error,
        nodes: result_nodes,
        edges: result_edges,
        outside: outside.len() as u32,
        outside_at,
    }
}

#[cfg(test)]
mod test;
