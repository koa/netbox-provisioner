//! The state of NetBox's edges (cables and wireless links) from the link-state database
//! (`docs/concepts/network-map.md`): OSPFv2 says whether the routers of both ends share the
//! segment of the near end's address, OSPFv3 is the second opinion.

use super::lsa::{Lsa, OspfVersion, RouterLink};
use crate::topology::{
    DeviceId, InterfaceId,
    access::{device::DeviceAccess, interface::InterfaceAccess},
};
use ipnet::{IpNet, Ipv4Net};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
};

/// The LSAs read from one router, as far as the edges need them. A router that went away
/// leaves its LSAs behind until they age out (an hour), so a segment counts only for the
/// routers whose own router LSA links to it, as in the SPF.
#[derive(Debug, Default, Clone)]
pub struct Lsdb {
    /// the originators of v2 router LSAs
    routers_v2: BTreeSet<Ipv4Addr>,
    /// the DRs each router's v2 router LSA has a transit link to
    transit_v2: HashMap<Ipv4Addr, HashSet<Ipv4Addr>>,
    /// the v2 segments: network, DR's address, routers
    segments_v2: Vec<(Ipv4Net, Ipv4Addr, BTreeSet<Ipv4Addr>)>,
    /// the neighbors (a segment's DR, a point-to-point peer) each router's v3 router LSA names
    neighbors_v3: HashMap<Ipv4Addr, HashSet<Ipv4Addr>>,
    /// the v3 segments: DR's router ID, routers
    segments_v3: Vec<(Ipv4Addr, BTreeSet<Ipv4Addr>)>,
}

impl FromIterator<Lsa> for Lsdb {
    fn from_iter<I: IntoIterator<Item = Lsa>>(lsas: I) -> Self {
        let mut lsdb = Lsdb::default();
        for lsa in lsas {
            let network = lsa.network();
            match lsa {
                Lsa::Router {
                    version: OspfVersion::V2,
                    originator,
                    links,
                } => {
                    lsdb.routers_v2.insert(originator);
                    let transit = lsdb.transit_v2.entry(originator).or_default();
                    for link in links {
                        if let RouterLink::Transit { dr, .. } = link {
                            transit.insert(dr);
                        }
                    }
                }
                Lsa::Router {
                    version: OspfVersion::V3,
                    originator,
                    links,
                } => {
                    let neighbors = lsdb.neighbors_v3.entry(originator).or_default();
                    for link in links {
                        if let RouterLink::Neighbor { router } = link {
                            neighbors.insert(router);
                        }
                    }
                }
                Lsa::Network {
                    version: OspfVersion::V2,
                    id,
                    routers,
                    ..
                } => {
                    if let Some(network) = network {
                        lsdb.segments_v2.push((network, id, routers));
                    }
                }
                Lsa::Network {
                    version: OspfVersion::V3,
                    originator,
                    routers,
                    ..
                } => lsdb.segments_v3.push((originator, routers)),
            }
        }
        lsdb
    }
}

impl Lsdb {
    /// The router IDs with a v2 router LSA (a router gone lately too, see [`Lsdb`]).
    pub fn routers(&self) -> &BTreeSet<Ipv4Addr> {
        &self.routers_v2
    }
    /// The routers `router` shares a v2 segment with, both linking to it.
    pub fn segment_neighbors(&self, router: Ipv4Addr) -> BTreeSet<Ipv4Addr> {
        self.segments_v2
            .iter()
            .filter(|(_, dr, routers)| routers.contains(&router) && self.transit(router, *dr))
            .flat_map(|(_, dr, routers)| {
                routers.iter().copied().filter(move |other| *other != router && self.transit(*other, *dr))
            })
            .collect()
    }
    /// Whether `router`'s v2 router LSA has a transit link to the segment with DR `dr`.
    fn transit(&self, router: Ipv4Addr, dr: Ipv4Addr) -> bool {
        self.transit_v2.get(&router).is_some_and(|drs| drs.contains(&dr))
    }
    fn names_v3(&self, router: Ipv4Addr, neighbor: Ipv4Addr) -> bool {
        self.neighbors_v3.get(&router).is_some_and(|n| n.contains(&neighbor))
    }
    /// Whether `a` and `b` are neighbors in OSPFv3: on a segment both link to, or naming each
    /// other point to point.
    fn neighbors_v3(&self, a: Ipv4Addr, b: Ipv4Addr) -> bool {
        self.segments_v3.iter().any(|(dr, routers)| {
            routers.contains(&a) && routers.contains(&b) && self.names_v3(a, *dr) && self.names_v3(b, *dr)
        }) || (self.names_v3(a, b) && self.names_v3(b, a))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Cable,
    Radio,
}

/// Why an edge is down in OSPFv2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownReason {
    /// the device's router ID is in no router LSA: it is gone from the area
    RouterMissing(DeviceId),
    /// no segment holds the near end's address (with its router linking to it): the router
    /// has the network as a stub, no neighbor answers there
    NoSegment,
    /// the near end's segment is there, without the far end's router
    NotOnSegment(DeviceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeState {
    /// both routers on the segment, in OSPFv2 and v3
    Up,
    /// up in OSPFv2, but the two are no neighbors in OSPFv3 (IPv6 off on one of them)
    V3Down,
    Down(DownReason),
    /// an end isn't an OSPF router with an address there (a radio bridge device on L2, a
    /// customer port, a device without RouterOS): the LSAs say nothing about it
    NotOspf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub kind: EdgeKind,
    pub a: InterfaceId,
    pub b: InterfaceId,
    pub state: EdgeState,
}

/// The edges at `devices` (a site's), each once: their cables to interfaces and their
/// wireless links, with the state the LSDB gives them.
pub fn edges(devices: &[DeviceAccess], lsdb: &Lsdb) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    for device in devices {
        for interface in device.interfaces().iter() {
            let far_ends = interface
                .connected_interfaces()
                .into_iter()
                .map(|far| (EdgeKind::Cable, far))
                .chain(interface.wireless_peer().map(|far| (EdgeKind::Radio, far)));
            for (kind, far) in far_ends {
                let key = (kind, interface.id().min(far.id()), interface.id().max(far.id()));
                if !seen.insert(key) {
                    continue;
                }
                edges.push(Edge {
                    kind,
                    a: interface.id(),
                    b: far.id(),
                    state: state(interface, &far, lsdb),
                });
            }
        }
    }
    edges
}

/// An end as OSPF sees it: the router ID of its device and the IPv4 addresses (with OSPF) of
/// the port or its bridge.
pub(crate) fn ospf_end(interface: &InterfaceAccess) -> Option<(DeviceId, Ipv4Addr, Vec<Ipv4Net>)> {
    let device = interface.device().filter(DeviceAccess::has_routeros)?;
    let Some(IpAddr::V4(router_id)) = device.primary_ip_v4().and_then(|ip| ip.addr()) else {
        return None;
    };
    let port = interface.bridge().unwrap_or_else(|| interface.clone());
    if !port.use_ospf() {
        return None;
    }
    let nets = port
        .ips()
        .iter()
        .filter_map(|ip| match ip.net() {
            Some(IpNet::V4(net)) => Some(net),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!nets.is_empty()).then_some((device.id(), router_id, nets))
}

fn state(a: &InterfaceAccess, b: &InterfaceAccess, lsdb: &Lsdb) -> EdgeState {
    let (Some((device_a, router_a, nets)), Some((device_b, router_b, _))) = (ospf_end(a), ospf_end(b))
    else {
        return EdgeState::NotOspf;
    };
    for (device, router) in [(device_a, router_a), (device_b, router_b)] {
        if !lsdb.routers_v2.contains(&router) {
            return EdgeState::Down(DownReason::RouterMissing(device));
        }
    }
    // the near end's segment, as its router links to it (a stale one doesn't count)
    let Some((_, dr, routers)) = lsdb.segments_v2.iter().find(|(segment, dr, routers)| {
        nets.iter().any(|net| segment.contains(&net.addr()))
            && routers.contains(&router_a)
            && lsdb.transit(router_a, *dr)
    }) else {
        return EdgeState::Down(DownReason::NoSegment);
    };
    if !routers.contains(&router_b) || !lsdb.transit(router_b, *dr) {
        return EdgeState::Down(DownReason::NotOnSegment(device_b));
    }
    let v3 = lsdb.neighbors_v3(router_a, router_b);
    if v3 { EdgeState::Up } else { EdgeState::V3Down }
}

#[cfg(test)]
mod test;
