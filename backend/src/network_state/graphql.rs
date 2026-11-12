//! Scope: The network map's GraphQL fields: nodes, edges and their details; not reading the devices
//! (`read`, `device_data`).
//!
//! `Query.topology.networkState`: a site's, or every site's, devices and edges with their state
//! from the LSDB, for the network map (`docs/concepts/network-map.md`, step 2). Fast: only the
//! LSDB, no device polled. `Query.topology.networkStateDetails` is health, resources and PoE
//! (steps 3 and 4), asked separately so the overview doesn't wait on a round trip to every
//! device.

use super::{
    device_data::{self, DeviceMapData},
    edges::{DownReason, EdgeKind, EdgeState, Lsdb, edges},
    read::{LsdbRead, read, readers},
};
use crate::graphql::scalars::ScalarU64;
use crate::{
    device::poe::{self, PoeProblem},
    topology::{
        DeviceId, PoeMode,
        access::device::DeviceAccess,
        findings::{DeviceRef, InterfaceRef, device_ref, interface_ref},
    },
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

/// `Query.topology.networkStateDetails`: health, resources and PoE edges of `network_state`'s
/// devices with OSPF of their own, read from each device directly (not the LSDB) and so slower;
/// asked separately so `networkState` doesn't wait on it.
#[derive(SimpleObject)]
pub struct NetworkStateDetails {
    nodes: Vec<MapNodeDetails>,
    /// PSE→PD arrows, one per port currently feeding or modeled to (`docs/concepts/network-map.md`,
    /// step 4), only where NetBox has a cable to the fed port
    poe_edges: Vec<MapPoeEdge>,
}

#[derive(SimpleObject)]
pub struct MapNodeDetails {
    device: DeviceRef,
    /// `/system/health/print`, generic, not judged (`docs/concepts/network-map.md`, step 3)
    health: Vec<MapHealth>,
    resources: Option<MapResources>,
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
    /// `device.site`'s name, for the frontend to group and label a site's devices on the
    /// all-sites map; none of `device_ref`'s business, since most of it has no use for a name.
    site_name: Option<String>,
    /// the device's published position (custom field `map_position`), if an admin ever
    /// published one; the frontend starts a node there instead of the layout's own guess,
    /// unless the browser's own, unpublished arrangement (`localStorage`) overrides it.
    position: Option<MapPosition>,
}

#[derive(SimpleObject)]
pub struct MapPosition {
    x: i32,
    y: i32,
}

#[derive(SimpleObject)]
pub struct MapHealth {
    name: String,
    value: String,
    unit: Option<String>,
}

#[derive(SimpleObject)]
pub struct MapResources {
    uptime: String,
    cpu_load: u32,
    free_memory: ScalarU64,
    total_memory: ScalarU64,
    free_hdd_space: ScalarU64,
    version: String,
}

impl From<&DeviceMapData> for (Vec<MapHealth>, Option<MapResources>) {
    fn from(data: &DeviceMapData) -> Self {
        let health = data
            .health
            .iter()
            .map(|row| MapHealth {
                name: row.name.to_string(),
                value: row.value.to_string(),
                unit: row.unit.as_deref().map(str::to_string),
            })
            .collect();
        let resources = data.resources.as_ref().map(|r| MapResources {
            uptime: r.uptime.to_string(),
            cpu_load: r.cpu_load,
            free_memory: r.free_memory.into(),
            total_memory: r.total_memory.into(),
            free_hdd_space: r.free_hdd_space.into(),
            version: r.version.to_string(),
        });
        (health, resources)
    }
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

/// A PSE→PD arrow: `pse` feeds `pd` over a cable NetBox has between them.
#[derive(SimpleObject)]
pub struct MapPoeEdge {
    pse: InterfaceRef,
    pd: InterfaceRef,
    /// `poe-out-status` as the device reports it
    status: Option<String>,
    /// `poe-out-power`, watts; only while `status` is `powered-on`
    power: Option<f64>,
    /// where the device contradicts NetBox's PoE mode (`device::poe::Feeding`)
    problem: Option<PoeProblem>,
}

/// One [`MapPoeEdge`] per port of `candidates` currently feeding, or modeled in NetBox to
/// (`device::poe::feeding`), with a cable to the fed port.
fn poe_edges(
    candidates: &[DeviceAccess],
    device_data: &BTreeMap<DeviceId, Arc<DeviceMapData>>,
) -> Vec<MapPoeEdge> {
    let mut found = Vec::new();
    for device in candidates {
        let Some(data) = device_data.get(&device.id()) else {
            continue;
        };
        for feeding in poe::feeding(device, &data.poe) {
            let Some(pd) = feeding.far else { continue };
            if feeding.mode != Some(PoeMode::Pse) && feeding.status.as_deref() != Some("powered-on")
            {
                continue;
            }
            found.push(MapPoeEdge {
                pse: feeding.port,
                pd,
                status: feeding.status.map(Into::into),
                power: feeding.power,
                problem: feeding.problem,
            });
        }
    }
    found
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
        || (device.has_routeros()
            && router_id(device).is_some()
            && device.interfaces().iter().any(|i| i.use_ospf()))
}

/// The devices to show and the OSPF ones reached ([`on_map_devices`]).
struct OnMap {
    on_map: BTreeSet<DeviceId>,
    /// those of the OSPF devices a run of plain devices confirms adjacent
    reached: BTreeSet<DeviceId>,
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
) -> OnMap {
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
        let boundary_routers = boundary
            .iter()
            .filter_map(|id| by_id.get(id))
            .filter_map(router_id)
            .collect::<Vec<_>>();
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
    OnMap { on_map, reached }
}

/// The state of `devices` (a site's, or every device, with RouterOS or not) as the LSDB of one
/// of them says. Devices without OSPF, and their edges, are left out. Fast: only the LSDB,
/// read from one router; see [`network_state_details`] for health, resources and PoE.
pub async fn network_state(devices: Vec<DeviceAccess>) -> NetworkState {
    let readers = readers(&devices);
    let lsdb_result = read(&readers).await;
    let (read_from, age, lsdb, error) = match lsdb_result {
        Ok(Some(LsdbRead { device, lsdb: fact })) => {
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
    let mut by_id = devices
        .iter()
        .map(|d| (d.id(), d.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = BTreeMap::<DeviceId, BTreeSet<DeviceId>>::new();
    for edge in &map_edges {
        let (Some(a), Some(b)) = (interface(edge.a), interface(edge.b)) else {
            continue;
        };
        let (Some(device_a), Some(device_b)) = (a.device(), b.device()) else {
            continue;
        };
        by_id
            .entry(device_a.id())
            .or_insert_with(|| device_a.clone());
        by_id
            .entry(device_b.id())
            .or_insert_with(|| device_b.clone());
        adjacency
            .entry(device_a.id())
            .or_default()
            .insert(device_b.id());
        adjacency
            .entry(device_b.id())
            .or_default()
            .insert(device_a.id());
    }
    let OnMap {
        on_map,
        reached: mut up,
    } = on_map_devices(&by_id, readers, lsdb, &adjacency);

    let mut nodes = BTreeMap::<DeviceId, DeviceAccess>::new();
    for device in by_id.values() {
        if on_map.contains(&device.id()) {
            nodes.insert(device.id(), device.clone());
        }
    }
    let mut result_edges = Vec::new();
    for edge in &map_edges {
        let (Some(a), Some(b)) = (interface(edge.a), interface(edge.b)) else {
            continue;
        };
        let (Some(device_a), Some(device_b)) = (a.device(), b.device()) else {
            continue;
        };
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
            EdgeState::Down(DownReason::RouterMissing(d)) => (
                MapEdgeState::Down,
                Some(MapDownReason::RouterMissing),
                Some(d.0),
            ),
            EdgeState::Down(DownReason::NoSegment) => {
                (MapEdgeState::Down, Some(MapDownReason::NoSegment), None)
            }
            EdgeState::Down(DownReason::NotOnSegment(d)) => (
                MapEdgeState::Down,
                Some(MapDownReason::NotOnSegment),
                Some(d.0),
            ),
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
                    _ if Some(device.id()) == reader || up.contains(&device.id()) => {
                        NodeState::Reached
                    }
                    Some(id) if lsdb.routers().contains(&id) => NodeState::Isolated,
                    _ => NodeState::Missing,
                }
            };
            MapNode {
                device: device_ref(device),
                routeros: device.has_routeros(),
                state,
                site_name: device.site().map(|s| s.name().to_string()),
                position: device.map_position().map(|position| MapPosition {
                    x: position.x,
                    y: position.y,
                }),
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
    let outside = lsdb
        .routers()
        .difference(&known)
        .copied()
        .collect::<BTreeSet<_>>();
    let outside_at = devices
        .iter()
        .filter(|device| {
            router_id(device).is_some_and(|id| {
                lsdb.segment_neighbors(id)
                    .iter()
                    .any(|n| outside.contains(n))
            })
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

/// Health, resources and PoE edges of `devices`' candidates (`supports_ospf`): slower than
/// [`network_state`] (one round trip per device), asked separately.
pub async fn network_state_details(devices: Vec<DeviceAccess>) -> NetworkStateDetails {
    let readers = readers(&devices);
    let candidates = devices
        .iter()
        .filter(|d| supports_ospf(d, &readers))
        .cloned()
        .collect::<Vec<_>>();
    let device_data = device_data::read_all(&candidates).await;
    build_details(&candidates, &device_data)
}

/// The synchronous part of [`network_state_details`]: testable without a device to read from.
fn build_details(
    candidates: &[DeviceAccess],
    device_data: &BTreeMap<DeviceId, Arc<DeviceMapData>>,
) -> NetworkStateDetails {
    let mut nodes = candidates
        .iter()
        .filter_map(|device| {
            let data = device_data.get(&device.id())?;
            let (health, resources) = (&**data).into();
            Some(MapNodeDetails {
                device: device_ref(device),
                health,
                resources,
            })
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|a, b| a.device.name.cmp(&b.device.name));
    let poe_edges = poe_edges(candidates, device_data);
    NetworkStateDetails { nodes, poe_edges }
}

#[cfg(test)]
mod test;
