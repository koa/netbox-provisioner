//! Scope: Parsing the OSPF link-state database; not judging it (`edges`).
//!
//! The OSPF link-state database as `/routing/ospf/lsa/print` gives it: a row per LSA, the
//! content in `body`, a first line of `key=value` and an indented line per link or router.
//!
//! Read on RouterOS 7.23 (lab-np2, CHR 7.24.5):
//!
//! ```text
//! router  v2  options=E bits=E
//!                 type=network id=172.17.1.82 data=172.17.1.82 metric=100
//!                 type=stub id=10.0.0.0 data=255.255.255.252 metric=100
//! network v2  netmask=255.255.255.252
//!                 router-id=172.16.1.1
//!                 router-id=192.168.100.3
//! router  v3  options=V6|E|R bits=E
//!                 type=network iface-id=7 neighbor-iface-if=7 neighbor-router-id=192.168.100.3 metric=100
//! network v3  options=V6|E|R
//!                 router-id=172.16.1.1
//! ```
//!
//! (`neighbor-iface-if` is RouterOS's spelling.) Other LSA types (`external`, `link`,
//! `intra-area-prefix`, …) are left out.

use ipnet::Ipv4Net;
use mikrotik_model::model::{OspfLsaType, RoutingOspfLsaState};
use std::{collections::BTreeSet, net::Ipv4Addr};

/// The OSPF instance an LSA is of, by its version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OspfVersion {
    V2,
    V3,
}

/// A link of a router LSA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterLink {
    /// v2 `type=network`: a broadcast segment with neighbors, by its DR's address (`id`) and the
    /// router's own address on it (`data`)
    Transit { dr: Ipv4Addr, own: Ipv4Addr },
    /// v2 `type=stub`: a network without neighbor
    Stub(Ipv4Net),
    /// v2 `type=p2p`: a point-to-point neighbor by its router ID
    PointToPoint { neighbor: Ipv4Addr },
    /// v3 `type=network` or `type=p2p`: a neighbor (the DR of a segment) by its router ID
    Neighbor { router: Ipv4Addr },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lsa {
    Router {
        version: OspfVersion,
        /// the router ID
        originator: Ipv4Addr,
        links: Vec<RouterLink>,
    },
    /// A broadcast segment and the routers on it
    Network {
        version: OspfVersion,
        /// the DR's router ID
        originator: Ipv4Addr,
        /// v2 the DR's address, v3 the DR's interface ID
        id: Ipv4Addr,
        /// v2 only
        netmask: Option<Ipv4Addr>,
        routers: BTreeSet<Ipv4Addr>,
    },
}

impl Lsa {
    /// One row of `/routing/ospf/lsa/print`; none for another type, another instance than
    /// `default-v2`/`default-v3` (the generator's) or one it can't read.
    pub fn from_state(lsa: &RoutingOspfLsaState) -> Option<Lsa> {
        let version = match &*lsa.instance.0 {
            b"default-v2" => OspfVersion::V2,
            b"default-v3" => OspfVersion::V3,
            _ => return None,
        };
        let body = lsa.body.to_string();
        let mut lines = body.lines().map(str::trim).filter(|line| !line.is_empty());
        let header = lines.next().unwrap_or_default();
        match lsa._type {
            OspfLsaType::Router => Some(Lsa::Router {
                version,
                originator: lsa.originator,
                links: lines
                    .filter_map(|line| router_link(version, line))
                    .collect(),
            }),
            OspfLsaType::Network => Some(Lsa::Network {
                version,
                originator: lsa.originator,
                id: lsa.id,
                netmask: value(header, "netmask").and_then(|mask| mask.parse().ok()),
                routers: lines
                    .filter_map(|line| value(line, "router-id"))
                    .filter_map(|id| id.parse().ok())
                    .collect(),
            }),
            _ => None,
        }
    }
    /// The segment of a v2 network LSA: the DR's address with the netmask.
    pub fn network(&self) -> Option<Ipv4Net> {
        match self {
            Lsa::Network {
                version: OspfVersion::V2,
                id,
                netmask: Some(netmask),
                ..
            } => Ipv4Net::with_netmask(*id, *netmask)
                .ok()
                .map(|net| net.trunc()),
            _ => None,
        }
    }
}

fn router_link(version: OspfVersion, line: &str) -> Option<RouterLink> {
    let addr = |key| value(line, key).and_then(|v| v.parse::<Ipv4Addr>().ok());
    match (version, value(line, "type")?) {
        (OspfVersion::V2, "network") => Some(RouterLink::Transit {
            dr: addr("id")?,
            own: addr("data")?,
        }),
        (OspfVersion::V2, "stub") => Ipv4Net::with_netmask(addr("id")?, addr("data")?)
            .ok()
            .map(RouterLink::Stub),
        (OspfVersion::V2, "p2p") => Some(RouterLink::PointToPoint {
            neighbor: addr("id")?,
        }),
        (OspfVersion::V3, "network" | "p2p") => Some(RouterLink::Neighbor {
            router: addr("neighbor-router-id")?,
        }),
        _ => None,
    }
}

/// The value of `key` in a line of `key=value` words.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .filter_map(|word| word.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

/// An LSA from the fields RouterOS reports, those always there but the ones given made up,
/// for tests.
#[cfg(test)]
pub(crate) fn lsa_state(fields: &[(&str, &str)]) -> RoutingOspfLsaState {
    let always = [
        ("sequence", "0x80000001"),
        ("age", "10"),
        ("checksum", "0x1234"),
        ("dynamic", "true"),
    ];
    crate::device::answer::parsed_with(fields, &always)
}

#[cfg(test)]
mod test;
