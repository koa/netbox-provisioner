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
    /// One row of `/routing/ospf/lsa/print` by its fields; none for another type, another
    /// instance than `default-v2`/`default-v3` (the generator's) or one it can't read.
    pub fn from_row(field: impl Fn(&str) -> Option<String>) -> Option<Lsa> {
        let version = match field("instance")?.as_str() {
            "default-v2" => OspfVersion::V2,
            "default-v3" => OspfVersion::V3,
            _ => return None,
        };
        let body = field("body").unwrap_or_default();
        let mut lines = body.lines().map(str::trim).filter(|line| !line.is_empty());
        let header = lines.next().unwrap_or_default();
        match field("type")?.as_str() {
            "router" => Some(Lsa::Router {
                version,
                originator: field("originator")?.parse().ok()?,
                links: lines
                    .filter_map(|line| router_link(version, line))
                    .collect(),
            }),
            "network" => Some(Lsa::Network {
                version,
                originator: field("originator")?.parse().ok()?,
                id: field("id")?.parse().ok()?,
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

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    pub(crate) fn row(fields: &[(&str, &str)]) -> Option<Lsa> {
        let fields: HashMap<_, _> = fields.iter().copied().collect();
        Lsa::from_row(|key| fields.get(key).map(|v| v.to_string()))
    }

    #[test]
    fn a_v2_router_lsa_has_its_transit_and_stub_links() {
        let lsa = row(&[
            ("instance", "default-v2"),
            ("type", "router"),
            ("originator", "192.168.100.3"),
            ("id", "192.168.100.3"),
            (
                "body",
                "options=E bits=E\n    type=network id=172.17.1.82 data=172.17.1.82 metric=100\n    type=stub id=10.0.50.0 data=255.255.255.248 metric=100\n",
            ),
        ]);
        assert_eq!(
            lsa,
            Some(Lsa::Router {
                version: OspfVersion::V2,
                originator: "192.168.100.3".parse().unwrap(),
                links: vec![
                    RouterLink::Transit {
                        dr: "172.17.1.82".parse().unwrap(),
                        own: "172.17.1.82".parse().unwrap()
                    },
                    RouterLink::Stub("10.0.50.0/29".parse().unwrap()),
                ],
            })
        );
    }

    #[test]
    fn a_v2_network_lsa_has_its_segment_and_routers() {
        let lsa = row(&[
            ("instance", "default-v2"),
            ("type", "network"),
            ("originator", "192.168.100.3"),
            ("id", "172.17.1.82"),
            (
                "body",
                "netmask=255.255.255.252\n    router-id=172.16.1.1\n    router-id=192.168.100.3\n",
            ),
        ])
        .unwrap();
        assert_eq!(lsa.network(), Some("172.17.1.80/30".parse().unwrap()));
        let Lsa::Network { routers, .. } = lsa else {
            panic!()
        };
        assert_eq!(
            routers,
            ["172.16.1.1", "192.168.100.3"]
                .iter()
                .map(|r| r.parse().unwrap())
                .collect()
        );
    }

    #[test]
    fn a_v3_router_lsa_names_its_neighbors() {
        let lsa = row(&[
            ("instance", "default-v3"),
            ("type", "router"),
            ("originator", "192.168.100.3"),
            ("id", "0.0.0.0"),
            (
                "body",
                "options=V6|E|R bits=E\n    type=network iface-id=7 neighbor-iface-if=7 neighbor-router-id=192.168.100.3 metric=100\n    type=network iface-id=109 neighbor-iface-if=4 neighbor-router-id=192.168.100.6 metric=100\n",
            ),
        ]);
        let Some(Lsa::Router { links, .. }) = lsa else {
            panic!("{lsa:?}")
        };
        assert_eq!(
            links,
            ["192.168.100.3", "192.168.100.6"]
                .iter()
                .map(|r| RouterLink::Neighbor {
                    router: r.parse().unwrap()
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn other_types_and_instances_are_left_out() {
        for (instance, kind) in [
            ("default-v2", "external"),
            ("default-v3", "link"),
            ("other", "router"),
        ] {
            assert_eq!(
                row(&[
                    ("instance", instance),
                    ("type", kind),
                    ("originator", "1.1.1.1"),
                    ("id", "1.1.1.1"),
                    ("body", "")
                ]),
                None
            );
        }
    }
}
