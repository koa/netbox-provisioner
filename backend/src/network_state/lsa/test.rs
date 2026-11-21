//! Scope: Tests of `lsa`; nothing else.
use super::*;

pub(crate) fn row(fields: &[(&str, &str)]) -> Option<Lsa> {
    Lsa::from_state(&lsa_state(fields))
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
        ("default-v2", "summary-network"),
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
