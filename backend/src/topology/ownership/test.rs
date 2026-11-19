//! Scope: Tests of `topology::ownership`; nothing else.
use crate::topology::test::{
    HUBER, MEIER, OwnedExample, example_response, owned_example, topology_from,
};
use crate::topology::{CablePort, DeviceId, FrontPortId, InterfaceId, OwnerId, VlanId, WlanId};
use std::collections::BTreeSet;

/// An owner has its devices with all their ports, the ports of other devices NetBox gives
/// it, its VLANs, the WLANs on them and their groups.
#[test]
fn an_owner_has_its_devices_ports_vlans_and_their_wlans() {
    let OwnedExample {
        raw,
        device,
        port,
        vlan,
        wlan,
    } = owned_example();
    let topology = topology_from(raw);
    let owned = topology.owned_by(MEIER);
    assert_eq!(owned.devices, BTreeSet::from([DeviceId(device)]));
    let device_ports =
        topology.devices[&DeviceId(device)]
            .ports
            .iter()
            .filter_map(|port| match port {
                CablePort::Interface(interface) => Some(*interface),
                _ => None,
            });
    let expected: BTreeSet<InterfaceId> = device_ports.chain([InterfaceId(port)]).collect();
    assert_eq!(owned.ports, expected);
    assert_eq!(owned.vlans, BTreeSet::from([VlanId(vlan)]));
    assert_eq!(owned.wlans, BTreeSet::from([WlanId(wlan)]));
    let group = topology.wlans[&WlanId(wlan)].wlan_group;
    assert_eq!(owned.wlan_groups, BTreeSet::from([group]));
}

/// Another owner's VLAN and its WLANs aren't the first one's, and the first one's are none
/// of the other's.
#[test]
fn owners_have_nothing_of_each_other() {
    let OwnedExample {
        raw,
        device,
        port,
        vlan,
        ..
    } = owned_example();
    let topology = topology_from(raw);
    let other = topology.owned_by(HUBER);
    assert!(other.devices.is_empty());
    assert!(other.ports.is_empty());
    assert_eq!(other.vlans.len(), 1);
    assert!(!other.vlans.contains(&VlanId(vlan)));
    assert!(!other.ports.contains(&InterfaceId(port)));
    assert!(topology.owned_by(MEIER).devices.contains(&DeviceId(device)));
    let names = topology
        .owners()
        .map(|(id, owner)| (id, owner.name.to_string()))
        .collect::<BTreeSet<_>>();
    assert!(
        names.contains(&(MEIER, "Familie Meier".into())),
        "{names:?}"
    );
    assert!(names.contains(&(HUBER, "Huber AG".into())), "{names:?}");
}

/// An owner no object names has nothing.
#[test]
fn an_unknown_owner_has_nothing() {
    let topology = topology_from(example_response());
    assert_eq!(topology.owned_by(OwnerId(9003)), Default::default());
}

/// The export's sockets: front ports NetBox gives an owner (rooms in a flat).
#[test]
fn the_exports_owned_front_ports_are_sockets() {
    let raw = example_response();
    let owned: BTreeSet<(u32, u32)> = raw["data"]["device_list"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|device| device["frontports"].as_array().into_iter().flatten())
        .filter_map(|port| {
            let owner = port["owner"]["id"].as_str()?.parse().ok()?;
            let id = port["id"].as_str()?.parse().ok()?;
            Some((owner, id))
        })
        .collect();
    let topology = topology_from(raw);
    for (owner, port) in &owned {
        let sockets = topology.owned_by(OwnerId(*owner)).sockets;
        assert!(sockets.contains(&FrontPortId(*port)), "{owner} {port}");
    }
}
