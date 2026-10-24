//! The live checks of the interface view (`docs/concepts/interface-view.md`): who the device
//! sees at each row's ports (`/ip/neighbor`, as „Nachbarn lesen“), its OSPF neighbors there, and
//! whether the bridge devices of a radio link answer a ping at their loopback. Read only; the
//! rows are those of `topology::interfaces`, so the frontend joins both by the row's interface.
//!
//! An OSPF neighbor names the interface as RouterOS calls it: a port by its short name and label
//! (`e01-sxt-a`), a bridge or VLAN interface by what L2 generated (`bridge-link` for NetBox'
//! `link`). So beyond the port names it goes by the addresses `/ip/address` has there.
use crate::{
    Error,
    device::{
        neighbors::{self, NeighborFinding, NeighborFindingKind, SeenNeighbor, port_named},
        raw::{Row, answering, field, rows},
        update_fallback::is_adjacent,
    },
    topology::{
        DeviceId,
        access::device::DeviceAccess,
        findings::{DeviceRef, device_ref},
        interfaces::{ActionHelper, ActionKind, InterfaceRow, InterfaceTask, RowAction},
    },
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{collections::BTreeSet, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceLive {
    pub rows: Vec<LiveRow>,
    /// the neighbor findings on no row (a port NetBox doesn't have, discovery off)
    pub other_findings: Vec<NeighborFinding>,
}

/// What the device shows at a row.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LiveRow {
    /// the row's interface (`InterfaceRow.interface`)
    pub interface: u32,
    /// who the device sees at its ports
    pub seen: Vec<SeenNeighbor>,
    /// its OSPF neighbors at the row's interface
    pub ospf: Vec<OspfNeighbor>,
    /// the routers NetBox puts at the other end (OSPF on the row) with no adjacency there
    pub ospf_missing: Vec<DeviceRef>,
    /// the bridge devices of a radio link, asked at their primary IPv4
    pub bridges: Vec<BridgeAnswer>,
    /// the neighbor findings about its ports, the worst first
    pub findings: Vec<NeighborFinding>,
    /// what the findings suggest: the link to the router the device sees at a port
    pub actions: Vec<RowAction>,
}

/// A neighbor of `/routing/ospf/neighbor`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct OspfNeighbor {
    pub instance: Box<str>,
    pub address: Box<str>,
    pub router_id: Box<str>,
    pub state: Box<str>,
    /// `Full`, or `TwoWay` between two DROthers: where OSPF leaves it
    pub adjacent: bool,
    /// the NetBox device whose primary IPv4 is its router ID (or which has its address)
    pub device: Option<DeviceRef>,
}

/// A bridge device of a radio link and whether it answered three pings from the device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct BridgeAnswer {
    pub device: DeviceRef,
    /// its primary IPv4; without one it isn't asked
    pub address: Option<Box<str>>,
    pub answers: bool,
}

/// Reads the device and checks its rows.
pub async fn read(client: &RawDevice<SimpleResult>, device: &DeviceAccess) -> Result<InterfaceLive, Error> {
    let neighbors = neighbors::read(client, device).await?;
    let ospf = rows(client, "/routing/ospf/neighbor/print").await?;
    let addresses = rows(client, "/ip/address/print").await?;
    let rows = device.interface_rows();
    let bridges = rows
        .iter()
        .flat_map(|(row, _)| bridge_devices(device, row))
        .collect::<BTreeSet<_>>();
    let loopbacks = bridges
        .iter()
        .filter_map(|id| device.topology().get_device_by_id(&DeviceId(*id))?.primary_ip_v4()?.addr())
        .collect::<Vec<_>>();
    let answering = answering(client, &loopbacks).await;
    Ok(check(device, &rows, neighbors, &ospf, &addresses, &answering))
}

/// [`read`] on what the device answered.
pub fn check(
    device: &DeviceAccess,
    rows: &[(InterfaceRow, BTreeSet<u32>)],
    neighbors: neighbors::Neighbors,
    ospf: &[Row],
    addresses: &[Row],
    answering: &BTreeSet<IpAddr>,
) -> InterfaceLive {
    let topology = device.topology();
    // the NetBox interface of a RouterOS interface name: the port of that name, else the one
    // with an address RouterOS has on it
    let netbox_interface = |name: &str| {
        port_named(device, name).map(|i| i.id().0).or_else(|| {
            let on_it = addresses
                .iter()
                .filter(|row| field(row, "interface") == Some(name))
                .filter_map(|row| field(row, "address")?.split('/').next()?.parse::<IpAddr>().ok())
                .collect::<Vec<_>>();
            device
                .interfaces()
                .iter()
                .find(|i| i.ips().iter().any(|ip| ip.addr().is_some_and(|ip| on_it.contains(&ip))))
                .map(|i| i.id().0)
        })
    };
    let ospf_device = |row: &Row| {
        let router_id = field(row, "router-id").unwrap_or_default();
        let address = field(row, "address").unwrap_or_default();
        topology
            .list_devices()
            .find(|d| d.primary_ip_v4().and_then(|ip| ip.addr()).is_some_and(|ip| ip.to_string() == router_id))
            .or_else(|| {
                topology.list_devices().find(|d| {
                    d.interfaces()
                        .iter()
                        .flat_map(|i| i.ips())
                        .any(|ip| ip.addr().is_some_and(|ip| ip.to_string() == address))
                })
            })
    };
    let mut placed = vec![false; neighbors.findings.len()];
    let live_rows = rows
        .iter()
        .map(|(row, members)| {
            let seen = members
                .iter()
                .flat_map(|id| neighbors.seen.get(id).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            let ospf_here = ospf
                .iter()
                .filter(|neighbor| {
                    field(neighbor, "interface")
                        .and_then(netbox_interface)
                        .is_some_and(|interface| members.contains(&interface))
                })
                .map(|neighbor| OspfNeighbor {
                    instance: field(neighbor, "instance").unwrap_or_default().into(),
                    address: field(neighbor, "address").unwrap_or_default().into(),
                    router_id: field(neighbor, "router-id").unwrap_or_default().into(),
                    state: field(neighbor, "state").unwrap_or_default().into(),
                    adjacent: is_adjacent(ospf, neighbor),
                    device: ospf_device(neighbor).as_ref().map(device_ref),
                })
                .collect::<Vec<_>>();
            let ospf_missing = expected_ospf(device, row)
                .into_iter()
                .filter(|expected| {
                    !ospf_here
                        .iter()
                        .any(|n| n.adjacent && n.device.as_ref().is_some_and(|d| d.id == expected.id))
                })
                .collect();
            let bridges = bridge_devices(device, row)
                .into_iter()
                .filter_map(|id| topology.get_device_by_id(&DeviceId(id)))
                .map(|bridge| {
                    let address = bridge.primary_ip_v4().and_then(|ip| ip.addr());
                    BridgeAnswer {
                        device: device_ref(&bridge),
                        address: address.map(|a| a.to_string().into()),
                        answers: address.is_some_and(|a| answering.contains(&a)),
                    }
                })
                .collect();
            let findings: Vec<NeighborFinding> = neighbors
                .findings
                .iter()
                .zip(placed.iter_mut())
                .filter(|(finding, _)| {
                    port_of(finding).is_some_and(|id| members.contains(&id))
                })
                .map(|(finding, placed)| {
                    *placed = true;
                    finding.clone()
                })
                .collect();
            let mut actions = Vec::<RowAction>::new();
            for action in findings.iter().filter_map(|finding| seen_link(device, finding)) {
                if !actions.contains(&action) {
                    actions.push(action);
                }
            }
            LiveRow {
                interface: row.interface.id,
                seen,
                ospf: ospf_here,
                ospf_missing,
                bridges,
                findings,
                actions,
            }
        })
        .collect();
    let other_findings = neighbors
        .findings
        .into_iter()
        .zip(placed)
        .filter(|(_, placed)| !placed)
        .map(|(finding, _)| finding)
        .collect();
    InterfaceLive {
        rows: live_rows,
        other_findings,
    }
}

/// The NetBox interface a neighbor finding is about.
fn port_of(finding: &NeighborFinding) -> Option<u32> {
    let port = match &finding.kind {
        NeighborFindingKind::CableMissing(f) => &f.port,
        NeighborFindingKind::CableElsewhere(f) => &f.port,
        NeighborFindingKind::OtherFarPort(f) => &f.port,
        NeighborFindingKind::NeighborNotSeen(f) => &f.port,
        NeighborFindingKind::NeighborsNotInNetbox(f) => &f.port,
        NeighborFindingKind::DiscoveryOff(_) => return None,
    };
    port.interface.as_ref().map(|interface| interface.id)
}

/// The link to the one router the device sees at a port where NetBox has no cable
/// („Verbindung“) or one elsewhere („Kabel übernehmen“). Radio bridge devices are left out:
/// their links are radio links.
fn seen_link(device: &DeviceAccess, finding: &NeighborFinding) -> Option<RowAction> {
    let (port, seen, helper) = match &finding.kind {
        NeighborFindingKind::CableMissing(f) => (&f.port, f.seen.as_slice(), ActionHelper::Connection),
        NeighborFindingKind::CableElsewhere(f) => (&f.port, f.seen.as_slice(), ActionHelper::CableTakeover),
        NeighborFindingKind::OtherFarPort(f) => (&f.port, std::slice::from_ref(&f.seen), ActionHelper::CableTakeover),
        _ => return None,
    };
    let [seen] = seen else { return None };
    let (port, peer) = (port.interface.as_ref()?, seen.interface.as_ref()?);
    let topology = device.topology();
    let peer_device = topology.get_device_by_id(&DeviceId(peer.device.id))?;
    let routers = [device, &peer_device];
    if routers.iter().any(|d| !d.has_routeros() || d.is_bridge_device()) {
        return None;
    }
    Some(RowAction {
        kind: ActionKind::Suggestion,
        helper,
        interface: Some(port.id),
        peer: Some(peer.clone()),
        vlan: None,
    })
}

/// The routers an `ospf` row should be adjacent with: the RouterOS devices at its cables, and on
/// a radio link every device of the path with `ospf` somewhere (the bridge devices are OSPF
/// routers through their bridge).
fn expected_ospf(device: &DeviceAccess, row: &InterfaceRow) -> Vec<DeviceRef> {
    if !row.ospf {
        return Vec::new();
    }
    let topology = device.topology();
    let ids = match (row.task, &row.radio_path) {
        (InterfaceTask::Direct, _) => row.far_ends.iter().map(|far| far.device.id).collect(),
        (InterfaceTask::RadioLink, Some(path)) => [
            &path.router_port,
            &path.bridge_port,
            &path.peer_bridge_port,
            &path.peer_router_port,
        ]
        .into_iter()
        .flatten()
        .map(|end| end.device.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect(),
        _ => Vec::new(),
    };
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| *id != device.id().0 && seen.insert(*id))
        .filter_map(|id| topology.get_device_by_id(&DeviceId(id)))
        .filter(|d| d.has_routeros() && d.interfaces().iter().any(|i| i.use_ospf()))
        .map(|d| device_ref(&d))
        .collect()
}

/// The bridge devices of a radio link row other than the device itself.
fn bridge_devices(device: &DeviceAccess, row: &InterfaceRow) -> Vec<u32> {
    let Some(path) = &row.radio_path else {
        return Vec::new();
    };
    [&path.bridge_port, &path.peer_bridge_port]
        .into_iter()
        .flatten()
        .map(|end| end.device.id)
        .filter(|id| *id != device.id().0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod test;
