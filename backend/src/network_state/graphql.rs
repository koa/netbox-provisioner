//! `Site.networkState`: the site's devices and edges with their state from the LSDB, for the
//! network map (`docs/concepts/network-map.md`, step 2).

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
    sync::Arc,
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
    /// no OSPF router (no RouterOS, no primary IPv4, no `ospf`)
    NoOspf,
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

/// The state of `devices` (a site's, with RouterOS or not) as the LSDB of one of them says.
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
    let empty = Arc::new(Lsdb::default());
    let lsdb = lsdb.unwrap_or(empty);
    let map_edges = edges(&devices, &lsdb);
    let topology = devices.first().map(|d| d.topology().clone());
    let interface = |id| topology.as_ref().and_then(|t| t.get_interface_by_id(id));

    // the devices at the edges, also those of another site
    let mut nodes = BTreeMap::<DeviceId, DeviceAccess>::new();
    for device in &devices {
        nodes.insert(device.id(), device.clone());
    }
    let mut up = BTreeSet::new();
    let mut result_edges = Vec::new();
    for edge in &map_edges {
        let (Some(a), Some(b)) = (interface(edge.a), interface(edge.b)) else { continue };
        for end in [&a, &b] {
            if let Some(device) = end.device() {
                if matches!(edge.state, EdgeState::Up | EdgeState::V3Down) {
                    up.insert(device.id());
                }
                nodes.entry(device.id()).or_insert(device);
            }
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
            let ospf = readers.iter().any(|r| r.id() == device.id())
                || (device.has_routeros()
                    && router_id(device).is_some()
                    && device.interfaces().iter().any(|i| i.use_ospf()));
            let state = match router_id(device) {
                _ if !ospf => NodeState::NoOspf,
                _ if Some(device.id()) == reader || up.contains(&device.id()) => NodeState::Reached,
                Some(id) if lsdb.routers().contains(&id) => NodeState::Isolated,
                _ => NodeState::Missing,
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
