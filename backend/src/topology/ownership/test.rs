//! Scope: Tests of `topology::ownership`; nothing else.
use crate::topology::test::{example_response, topology_from};
use crate::topology::{CablePort, DeviceId, InterfaceId, OwnerId, VlanId, WlanId};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn id(value: &Value) -> u32 {
    value["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("id")
}

/// A VLAN with a WLAN on it in the export, and that WLAN.
fn vlan_with_wlan(raw: &Value) -> (u32, u32) {
    raw["data"]["wireless_lan_group_list"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["wireless_lans"].as_array().into_iter().flatten())
        .find(|wlan| wlan["vlan"].is_object())
        .map(|wlan| (id(&wlan["vlan"]), id(wlan)))
        .expect("a WLAN on a VLAN")
}

fn set_owner(object: &mut Value, owner: u32, name: &str) {
    object["owner"] = json!({ "id": owner.to_string(), "name": name });
}

/// The export with owner 1 at a device, at a port of another device and at a VLAN with a
/// WLAN; owner 2 at another VLAN. Gives the device, the port, the VLAN and its WLAN.
fn owned_export() -> (Value, u32, u32, u32, u32) {
    let mut raw = example_response();
    let (vlan, wlan) = vlan_with_wlan(&raw);
    let devices = raw["data"]["device_list"].as_array_mut().expect("devices");
    let mut with_ports = devices.iter_mut().filter(|device| {
        device["interfaces"]
            .as_array()
            .is_some_and(|i| i.len() >= 2)
    });
    let device = with_ports.next().expect("a device");
    set_owner(device, 1, "Familie Meier");
    let device_id = id(device);
    let other = with_ports.next().expect("another device");
    let port = &mut other["interfaces"][0];
    set_owner(port, 1, "Familie Meier");
    let port_id = id(port);
    let mut other_vlan_owned = false;
    for group in raw["data"]["vlan_group_list"]
        .as_array_mut()
        .expect("groups")
    {
        for candidate in group["vlans"].as_array_mut().into_iter().flatten() {
            if id(candidate) == vlan {
                set_owner(candidate, 1, "Familie Meier");
            } else if !other_vlan_owned {
                set_owner(candidate, 2, "Huber AG");
                other_vlan_owned = true;
            }
        }
    }
    assert!(other_vlan_owned, "a second VLAN in the export");
    (raw, device_id, port_id, vlan, wlan)
}

/// An owner has its devices with all their ports, the ports of other devices NetBox gives
/// it, its VLANs, the WLANs on them and their groups.
#[test]
fn an_owner_has_its_devices_ports_vlans_and_their_wlans() {
    let (raw, device, port, vlan, wlan) = owned_export();
    let topology = topology_from(raw);
    let owned = topology.owned_by(OwnerId(1));
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
    let (raw, device, port, vlan, _) = owned_export();
    let topology = topology_from(raw);
    let other = topology.owned_by(OwnerId(2));
    assert!(other.devices.is_empty());
    assert!(other.ports.is_empty());
    assert_eq!(other.vlans.len(), 1);
    assert!(!other.vlans.contains(&VlanId(vlan)));
    assert!(!other.ports.contains(&InterfaceId(port)));
    assert!(
        topology
            .owned_by(OwnerId(1))
            .devices
            .contains(&DeviceId(device))
    );
    let names = topology
        .owners()
        .map(|(id, owner)| (id, owner.name.to_string()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            (OwnerId(1), "Familie Meier".into()),
            (OwnerId(2), "Huber AG".into())
        ])
    );
}

/// An owner no object names has nothing; the export as stored has no owners.
#[test]
fn without_owners_nothing_is_owned() {
    let topology = topology_from(example_response());
    assert_eq!(topology.owners().count(), 0);
    assert_eq!(topology.owned_by(OwnerId(1)), Default::default());
}
