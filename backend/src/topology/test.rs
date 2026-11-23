//! Scope: The fixtures several modules' tests share (the stored export as a topology); not tests
//! themselves.
//!
//! Fixtures for tests across the crate.

use crate::{
    netbox::fetch_topology::FetchTopology,
    topology::{Topology, fetch::build_topology_from},
};
use std::sync::Arc;
use tokio::time::Instant;

/// The topology of `docs/netbox-response-example.json`, with what older exports lack
/// filled in as empty.
pub(crate) fn example_topology() -> Arc<Topology> {
    topology_from(example_response())
}

/// The topology of a NetBox response as [`example_response`] gives it.
pub(crate) fn topology_from(raw: serde_json::Value) -> Arc<Topology> {
    let response: cynic::GraphQlResponse<FetchTopology> =
        serde_json::from_value(raw).expect("parse example response");
    Arc::new(build_topology_from(
        response.data.expect("response without data"),
        Instant::now(),
        None,
    ))
}

/// `docs/netbox-response-example.json` with what older exports lack filled in as empty, for
/// a test to change before [`topology_from`].
pub(crate) fn example_response() -> serde_json::Value {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/netbox-response-example.json"
    ))
    .expect("example response");
    let mut raw: serde_json::Value = serde_json::from_str(&raw).expect("example response json");
    // Exports from before VLAN groups were queried with their custom fields lack
    // the field; treat those groups as having none (no VNI, nothing tunneled).
    for group in raw["data"]["vlan_group_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(group) = group.as_object_mut() {
            group
                .entry("custom_field_data")
                .or_insert_with(|| serde_json::json!({}));
        }
    }
    // Exports from before the wireless links were queried with their id and ends lack them:
    // the two ends of a link are the two interfaces with its SSID, without one the devices
    // named after each other (`hegglin-schopf`, `schopf-hegglin`); one alone has no peer.
    let mut ends = std::collections::BTreeMap::<String, Vec<String>>::new();
    for device in raw["data"]["device_list"].as_array().into_iter().flatten() {
        for interface in device["interfaces"].as_array().into_iter().flatten() {
            let link = &interface["wireless_link"];
            if link.is_object() && link.get("id").is_none() {
                let id = interface["id"].as_str().unwrap_or_default().to_string();
                let key = match link["ssid"].as_str().unwrap_or_default() {
                    "" => {
                        let mut parts = device["name"]
                            .as_str()
                            .unwrap_or_default()
                            .split('-')
                            .collect::<Vec<_>>();
                        parts.sort();
                        format!("#{}", parts.join("-"))
                    }
                    ssid => ssid.to_string(),
                };
                ends.entry(key).or_default().push(id);
            }
        }
    }
    let synthetic = ends
        .values()
        .enumerate()
        .flat_map(|(index, ids)| {
            let (a, b) = (ids[0].clone(), ids.get(1).unwrap_or(&ids[0]).clone());
            ids.iter()
                .map(move |id| (id.clone(), (900_000 + index, a.clone(), b.clone())))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
            let id = interface["id"].as_str().unwrap_or_default().to_string();
            if let Some((link_id, a, b)) = synthetic.get(&id) {
                let link = &mut interface["wireless_link"];
                link["id"] = serde_json::json!(link_id.to_string());
                link["interface_a"] = serde_json::json!({ "id": a });
                link["interface_b"] = serde_json::json!({ "id": b });
            }
        }
    }
    // The export has the wireless links' PSKs removed: an encrypted link gets a placeholder,
    // as the provisioner would get the real one.
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
            let link = &mut interface["wireless_link"];
            let encrypted = link["auth_type"]
                .as_str()
                .is_some_and(|auth| !auth.is_empty() && auth != "open");
            if encrypted && link["auth_psk"].as_str().is_none_or(str::is_empty) {
                link["auth_psk"] = serde_json::json!("removed-from-export");
            }
            // the PSK left out of the query when exporting: an open link has none
            if let Some(link) = link.as_object_mut() {
                link.entry("auth_psk")
                    .or_insert_with(|| serde_json::json!(""));
            }
        }
    }
    // and the WLANs' PSKs are left out of the query when exporting: a placeholder too
    for group in raw["data"]["wireless_lan_group_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for wlan in group["wireless_lans"].as_array_mut().into_iter().flatten() {
            if let Some(wlan) = wlan.as_object_mut() {
                wlan.entry("auth_psk")
                    .or_insert_with(|| serde_json::json!("removed-from-export"));
                // exports from before the status was queried: on
                wlan.entry("status")
                    .or_insert_with(|| serde_json::json!("active"));
            }
        }
    }
    // Exports from before circuits were queried lack them, and the ends of the circuits on
    // the cables their id: those cables end there, as they did then.
    if let Some(data) = raw["data"].as_object_mut() {
        data.entry("circuit_list")
            .or_insert_with(|| serde_json::json!([]));
        // and before the services were: none forwards a port
        data.entry("service_list")
            .or_insert_with(|| serde_json::json!([]));
    }
    // and before the locations were: a device's location without its id is none
    if let Some(data) = raw["data"].as_object_mut()
        && !data.contains_key("location_list")
    {
        data.insert("location_list".into(), serde_json::json!([]));
        for device in data["device_list"].as_array_mut().into_iter().flatten() {
            device["location"] = serde_json::Value::Null;
        }
    }
    // and before a location's site and tenant were: the site of a device in it (else the
    // first device's), no tenant
    let mut site_of = std::collections::HashMap::new();
    for device in raw["data"]["device_list"].as_array().into_iter().flatten() {
        if let Some(location) = device["location"]["id"].as_str() {
            site_of
                .entry(location.to_owned())
                .or_insert_with(|| device["site"]["id"].clone());
        }
    }
    let first_site = raw["data"]["device_list"][0]["site"]["id"].clone();
    for location in raw["data"]["location_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let site = location["id"]
            .as_str()
            .and_then(|id| site_of.get(id))
            .unwrap_or(&first_site)
            .clone();
        if let Some(location) = location.as_object_mut() {
            location
                .entry("site")
                .or_insert_with(|| serde_json::json!({ "id": site }));
            location.entry("tenant").or_insert(serde_json::Value::Null);
        }
    }
    for cable in raw["data"]["cable_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for side in ["a_terminations", "b_terminations"] {
            if let Some(ends) = cable[side].as_array_mut() {
                ends.retain(|end| {
                    end["__typename"] != "CircuitTerminationType" || end.get("id").is_some()
                });
            }
        }
    }
    // Exports from before the config context was queried lack it; treat those devices as
    // having none (no DNS servers configured).
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(device) = device.as_object_mut() {
            device
                .entry("config_context")
                .or_insert_with(|| serde_json::json!({}));
            // and before the sites' custom fields were: none rolls out automatically
            if let Some(site) = device.get_mut("site").and_then(|site| site.as_object_mut()) {
                site.entry("custom_field_data")
                    .or_insert_with(|| serde_json::json!({}));
            }
        }
    }
    // Exports from before prefixes were queried with tags, scope, description and custom
    // fields, and devices with their device type, lack them; treat those as empty.
    for prefix in raw["data"]["prefix_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(prefix) = prefix.as_object_mut() {
            prefix
                .entry("tags")
                .or_insert_with(|| serde_json::json!([]));
            prefix
                .entry("description")
                .or_insert_with(|| serde_json::json!(""));
            prefix
                .entry("custom_field_data")
                .or_insert_with(|| serde_json::json!({}));
        }
    }
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(device) = device.as_object_mut() {
            device
                .entry("device_type")
                .or_insert_with(|| serde_json::json!({"model": "", "manufacturer": {"slug": ""}}));
        }
    }
    // Exports from before the owners were queried lack them: nothing belongs to a resident.
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
            if let Some(interface) = interface.as_object_mut() {
                interface.entry("owner").or_insert(serde_json::Value::Null);
                // and from before the assigned MACs were: only the primary one
                let primary = interface
                    .get("primary_mac_address")
                    .filter(|mac| mac.is_object())
                    .cloned();
                interface
                    .entry("mac_addresses")
                    .or_insert_with(|| serde_json::json!(primary.into_iter().collect::<Vec<_>>()));
            }
        }
        // and the front ports' labels
        for front_port in device["frontports"].as_array_mut().into_iter().flatten() {
            if let Some(front_port) = front_port.as_object_mut() {
                front_port.entry("owner").or_insert(serde_json::Value::Null);
                front_port.entry("label").or_insert(serde_json::json!(""));
            }
        }
        if let Some(device) = device.as_object_mut() {
            device.entry("owner").or_insert(serde_json::Value::Null);
        }
    }
    for group in raw["data"]["vlan_group_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        for vlan in group["vlans"].as_array_mut().into_iter().flatten() {
            if let Some(vlan) = vlan.as_object_mut() {
                vlan.entry("owner").or_insert(serde_json::Value::Null);
            }
        }
    }
    raw
}

/// The owners [`owned_example`] adds, with ids NetBox doesn't give (the export has owners of
/// its own).
pub(crate) const MEIER: crate::topology::OwnerId = crate::topology::OwnerId(9001);
pub(crate) const HUBER: crate::topology::OwnerId = crate::topology::OwnerId(9002);

/// The stored export with owners: [`MEIER`] („Familie Meier“) at a device, at a port of another
/// device and at a VLAN with a WLAN, [`HUBER`] („Huber AG“) at another VLAN.
pub(crate) struct OwnedExample {
    pub raw: serde_json::Value,
    /// Meier's device
    pub device: u32,
    /// Meier's port of another device
    pub port: u32,
    /// Meier's VLAN, with `wlan` on it
    pub vlan: u32,
    pub wlan: u32,
}

pub(crate) fn owned_example() -> OwnedExample {
    fn id(value: &serde_json::Value) -> u32 {
        value["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("id")
    }
    fn set_owner(object: &mut serde_json::Value, owner: crate::topology::OwnerId, name: &str) {
        object["owner"] = serde_json::json!({ "id": owner.0.to_string(), "name": name });
    }
    let mut raw = example_response();
    let (vlan, wlan) = raw["data"]["wireless_lan_group_list"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["wireless_lans"].as_array().into_iter().flatten())
        .find(|wlan| wlan["vlan"].is_object())
        .map(|wlan| (id(&wlan["vlan"]), id(wlan)))
        .expect("a WLAN on a VLAN");
    let devices = raw["data"]["device_list"].as_array_mut().expect("devices");
    let mut with_ports = devices.iter_mut().filter(|device| {
        device["interfaces"]
            .as_array()
            .is_some_and(|interfaces| interfaces.len() >= 2)
    });
    let device = with_ports.next().expect("a device");
    set_owner(device, MEIER, "Familie Meier");
    let device_id = id(device);
    let other = with_ports.next().expect("another device");
    let port = &mut other["interfaces"][0];
    set_owner(port, MEIER, "Familie Meier");
    let port_id = id(port);
    let mut other_vlan_owned = false;
    for group in raw["data"]["vlan_group_list"]
        .as_array_mut()
        .expect("groups")
    {
        for candidate in group["vlans"].as_array_mut().into_iter().flatten() {
            if id(candidate) == vlan {
                set_owner(candidate, MEIER, "Familie Meier");
            } else if !other_vlan_owned {
                set_owner(candidate, HUBER, "Huber AG");
                other_vlan_owned = true;
            }
        }
    }
    assert!(other_vlan_owned, "a second VLAN in the export");
    OwnedExample {
        raw,
        device: device_id,
        port: port_id,
        vlan,
        wlan,
    }
}

/// A device's location is the path of names from the outermost one; a loop in NetBox's
/// parents ends it instead of running on.
#[test]
fn a_devices_location_is_its_path_of_locations() {
    let mut raw = example_response();
    let data = &mut raw["data"];
    data["location_list"] = serde_json::json!([
        { "id": "1", "name": "2. OG", "parent": null, "site": { "id": "1" }, "tenant": null },
        { "id": "2", "name": "Wohnung Meier", "parent": { "id": "1" }, "site": { "id": "1" },
          "tenant": null },
        { "id": "3", "name": "a", "parent": { "id": "4" }, "site": { "id": "1" }, "tenant": null },
        { "id": "4", "name": "b", "parent": { "id": "3" }, "site": { "id": "1" }, "tenant": null },
    ]);
    let devices = data["device_list"].as_array_mut().expect("devices");
    devices[0]["location"] = serde_json::json!({ "id": "2", "tenant": null });
    devices[1]["location"] = serde_json::json!({ "id": "3", "tenant": null });
    let first = devices[0]["name"].as_str().expect("a name").to_owned();
    let second = devices[1]["name"].as_str().expect("a name").to_owned();
    let topology = topology_from(raw);
    let location = |name: &str| {
        let device = topology
            .list_devices()
            .find(|device| device.name() == name)
            .expect("the device");
        device.location().to_vec()
    };
    assert_eq!(location(&first), ["2. OG".into(), "Wohnung Meier".into()]);
    assert!(location(&second).len() <= 4, "{:?}", location(&second));
}
