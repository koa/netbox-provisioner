//! Scope: Tests of `update_fallback`; nothing else.
use super::{adjacent, missing, script};
use crate::device::answer::parsed;
use mikrotik_model::model::RoutingOspfNeighborState;
use std::{collections::BTreeSet, time::Duration};

#[test]
fn switches_back_unless_removed() {
    assert_eq!(
        script("part1", Duration::from_secs(900)),
        ":delay 900s; :if ([:len [/system scheduler find name=\"netbox-provisioner-update-fallback\"]] > 0) \
             do={ /system scheduler remove [find name=\"netbox-provisioner-update-fallback\"]; \
             /partitions activate [find name=\"part1\"]; /system reboot }"
    );
}

#[test]
fn misses_only_neighbors_that_were_adjacent() {
    let set = |neighbors: &[&str]| {
        neighbors
            .iter()
            .map(|&n| n.into())
            .collect::<BTreeSet<Box<str>>>()
    };
    let before = set(&["default-v2 10.0.0.1", "default-v3 10.0.0.1"]);
    let now = set(&["default-v2 10.0.0.1", "default-v2 10.0.0.9"]);
    assert_eq!(
        missing(&before, &now),
        vec![Box::from("default-v3 10.0.0.1")]
    );
    assert!(missing(&set(&[]), &now).is_empty());
}

fn neighbor(
    instance: &str,
    address: &str,
    router_id: &str,
    dr: &str,
    bdr: &str,
    state: &str,
) -> RoutingOspfNeighborState {
    parsed(&[
        (".id", "*1"),
        ("instance", instance),
        ("area", "backbone"),
        ("interface", "bridge-bridge"),
        ("address", address),
        ("router-id", router_id),
        ("dr", dr),
        ("bdr", bdr),
        ("state", state),
    ])
}

/// berghof-stall-haus after its update (7.23.8): v3 elected 172.16.1.15 and 172.16.1.7, so
/// 172.16.1.60, `Full` before, is a DROther like this device and stays in `TwoWay`.
#[test]
fn two_drothers_in_two_way_are_adjacent() {
    let v3 = |address, router_id, state| {
        neighbor(
            "default-v3",
            address,
            router_id,
            "172.16.1.15",
            "172.16.1.7",
            state,
        )
    };
    let rows = [
        neighbor(
            "default-v2",
            "10.198.1.1",
            "172.16.1.7",
            "10.198.1.1",
            "10.198.1.3",
            "Full",
        ),
        neighbor(
            "default-v2",
            "10.198.1.3",
            "172.16.1.60",
            "10.198.1.1",
            "10.198.1.3",
            "Full",
        ),
        v3("fe80::1", "172.16.1.7", "Full"),
        v3("fe80::2", "172.16.1.15", "Full"),
        v3("fe80::3", "172.16.1.60", "TwoWay"),
    ];
    assert_eq!(
        adjacent(&rows),
        [
            "default-v2 172.16.1.60",
            "default-v2 172.16.1.7",
            "default-v3 172.16.1.15",
            "default-v3 172.16.1.60",
            "default-v3 172.16.1.7"
        ]
        .map(Box::<str>::from)
        .into()
    );
}

/// `TwoWay` with the DR or BDR, or where this device is one, is a stuck adjacency.
#[test]
fn two_way_with_the_dr_is_not_adjacent() {
    // the neighbor is the DR
    let rows = [neighbor(
        "default-v2",
        "10.0.0.1",
        "1.1.1.1",
        "10.0.0.1",
        "0.0.0.0",
        "TwoWay",
    )];
    assert!(adjacent(&rows).is_empty());
    // the DR is no neighbor: this device
    let rows = [neighbor(
        "default-v2",
        "10.0.0.2",
        "2.2.2.2",
        "10.0.0.9",
        "0.0.0.0",
        "TwoWay",
    )];
    assert!(adjacent(&rows).is_empty());
    let rows = [neighbor(
        "default-v2",
        "10.0.0.2",
        "2.2.2.2",
        "10.0.0.1",
        "10.0.0.2",
        "ExStart",
    )];
    assert!(adjacent(&rows).is_empty());
}
