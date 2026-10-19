//! Fixtures for tests across the crate.

use crate::{
    netbox::fetch_topology::ResponseData,
    topology::{Topology, fetch::build_topology_from},
};
use std::sync::Arc;
use tokio::time::Instant;

/// The topology of `docs/netbox-response-example.json`, with what older exports lack
/// filled in as empty.
pub(crate) fn example_topology() -> Arc<Topology> {
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
            ids.iter().map(move |id| (id.clone(), (900_000 + index, a.clone(), b.clone())))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for device in raw["data"]["device_list"].as_array_mut().into_iter().flatten() {
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
    for device in raw["data"]["device_list"].as_array_mut().into_iter().flatten() {
        for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
            let link = &mut interface["wireless_link"];
            let encrypted = link["auth_type"]
                .as_str()
                .is_some_and(|auth| !auth.is_empty() && auth != "open");
            if encrypted && link["auth_psk"].as_str().is_none_or(str::is_empty) {
                link["auth_psk"] = serde_json::json!("removed-from-export");
            }
        }
    }
    // Exports from before circuits were queried lack them, and the ends of the circuits on
    // the cables their id: those cables end there, as they did then.
    if let Some(data) = raw["data"].as_object_mut() {
        data.entry("circuit_list").or_insert_with(|| serde_json::json!([]));
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
        }
    }
    // Exports from before prefixes were queried with tags, scope and description, and devices
    // with their device type, lack them; treat those as empty.
    for prefix in raw["data"]["prefix_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(prefix) = prefix.as_object_mut() {
            prefix.entry("tags").or_insert_with(|| serde_json::json!([]));
            prefix.entry("description").or_insert_with(|| serde_json::json!(""));
        }
    }
    for device in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(device) = device.as_object_mut() {
            device.entry("device_type").or_insert_with(
                || serde_json::json!({"model": "", "manufacturer": {"slug": ""}}),
            );
        }
    }
    let response: graphql_client::Response<ResponseData> =
        serde_json::from_value(raw).expect("parse example response");
    Arc::new(build_topology_from(
        response.data.expect("response without data"),
        Instant::now(),
    ))
}
