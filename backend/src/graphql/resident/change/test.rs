//! Scope: Tests of `graphql::resident::change`; nothing else.
use crate::{
    graphql::resident::create_resident_schema,
    helper::connect_device::uplink_port,
    role::Role,
    topology::{
        TopologyHolder,
        test::{HUBER, MEIER, OwnedExample, owned_example, topology_from},
    },
};
use serde_json::{Value, json};

/// Asks the residents' schema `query` as `role`: the data, or the first error's code.
async fn ask(role: Role, query: &str) -> Result<Value, String> {
    let OwnedExample { raw, .. } = owned_example();
    ask_on(raw, role, query).await
}

/// [`ask`] on the NetBox response `raw`.
async fn ask_on(raw: Value, role: Role, query: &str) -> Result<Value, String> {
    let schema = create_resident_schema(TopologyHolder::from(topology_from(raw)));
    let response = schema
        .execute(async_graphql::Request::new(query).data(role))
        .await;
    match response.errors.first() {
        Some(error) => Err(error
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get("code"))
            .map(|code| code.to_string())
            .unwrap_or_else(|| error.message.clone())),
        None => Ok(response.data.into_json().expect("json")),
    }
}

fn plan_query(resident: u32, wlan: u32, ssid: &str) -> String {
    format!(
        r#"{{ changePlan(resident: {resident}, changes: [{{ wlan: {{ wlan: {wlan}, ssid: "{ssid}" }} }}]) {{
            fingerprint wlans {{ id ssid ssidBefore pskSet }} devices {{ id name }}
        }} }}"#
    )
}

/// A resident gets the plan for its own WLAN, with the devices that change: the group's
/// controller first.
#[tokio::test]
async fn a_resident_plans_a_change_of_its_wlan() {
    let OwnedExample { wlan, .. } = owned_example();
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    let data = ask(meier, &plan_query(MEIER.0, wlan, "meier-neu"))
        .await
        .expect("a plan");
    let plan = &data["changePlan"];
    assert_eq!(
        plan["wlans"],
        json!([{ "id": wlan, "ssid": "meier-neu", "ssidBefore": plan["wlans"][0]["ssidBefore"], "pskSet": false }])
    );
    assert!(plan["fingerprint"].as_str().is_some_and(|f| !f.is_empty()));
    assert!(
        plan["devices"]
            .as_array()
            .is_some_and(|devices| !devices.is_empty()),
        "{plan}"
    );
}

/// Nobody plans for an owner it isn't, nor for a WLAN its owner doesn't have; an admin plans
/// for any resident, within what that one has.
#[tokio::test]
async fn others_wlans_and_owners_are_refused() {
    let OwnedExample { wlan, .. } = owned_example();
    let huber = Role::Resident {
        owners: Box::new([HUBER]),
    };
    let not_owned = Err("\"NOT_OWNED\"".to_string());
    assert_eq!(
        ask(huber.clone(), &plan_query(MEIER.0, wlan, "x")).await,
        not_owned
    );
    assert_eq!(ask(huber, &plan_query(HUBER.0, wlan, "x")).await, not_owned);
    assert_eq!(
        ask(Role::Nobody, &plan_query(MEIER.0, wlan, "x")).await,
        not_owned
    );
    assert!(
        ask(Role::Admin, &plan_query(MEIER.0, wlan, "x"))
            .await
            .is_ok()
    );
    assert_eq!(
        ask(Role::Admin, &plan_query(HUBER.0, wlan, "x")).await,
        not_owned
    );
}

/// A job nobody saved a change with is none, for anyone.
#[tokio::test]
async fn an_unknown_job_is_no_saved_change() {
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    assert_eq!(
        ask(meier, "{ savedChange(job: 4711) { state } }").await,
        Ok(json!({ "savedChange": null }))
    );
}

fn port_query(resident: u32, port: u32, kind: &str, vlan: &str) -> String {
    format!(
        r#"{{ changePlan(resident: {resident}, changes: [{{ portVlan: {{ port: {port}, kind: {kind}, vlan: {vlan} }} }}]) {{
            fingerprint ports {{ vlan {{ vid }} }} devices {{ name }}
        }} }}"#
    )
}

/// A port, socket or VLAN not the resident's is refused before any plan.
#[tokio::test]
async fn others_ports_and_vlans_are_refused() {
    let OwnedExample { port, vlan, .. } = owned_example();
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    let huber = Role::Resident {
        owners: Box::new([HUBER]),
    };
    let not_owned = Err("\"NOT_OWNED\"".to_string());
    let vlan = vlan.to_string();
    assert_eq!(
        ask(huber, &port_query(HUBER.0, port, "INTERFACE", &vlan)).await,
        not_owned
    );
    assert_eq!(
        ask(
            meier.clone(),
            &port_query(MEIER.0, port, "INTERFACE", "4711")
        )
        .await,
        not_owned
    );
    assert_eq!(
        ask(meier, &port_query(MEIER.0, port, "SOCKET", &vlan)).await,
        not_owned
    );
}

fn connect_query(resident: u32, device: u32, port: u32, kind: &str) -> String {
    format!(
        r#"{{ changePlan(resident: {resident}, changes: [{{ connectDevice: {{ device: {device}, port: {port}, kind: {kind} }} }}]) {{
            fingerprint devices {{ name }}
        }} }}"#
    )
}

fn disconnect_query(resident: u32, device: u32) -> String {
    format!(
        r#"{{ changePlan(resident: {resident}, changes: [{{ disconnectDevice: {device} }}]) {{
            fingerprint
        }} }}"#
    )
}

/// Another's device, or one's own device at another's port or socket, is refused before any
/// plan; so is disconnecting another's device.
#[tokio::test]
async fn others_devices_and_ports_are_neither_connected_nor_disconnected() {
    let OwnedExample { device, port, .. } = owned_example();
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    let huber = Role::Resident {
        owners: Box::new([HUBER]),
    };
    let not_owned = Err("\"NOT_OWNED\"".to_string());
    assert_eq!(
        ask(
            huber.clone(),
            &connect_query(HUBER.0, device, port, "INTERFACE")
        )
        .await,
        not_owned
    );
    assert_eq!(
        ask(
            meier.clone(),
            &connect_query(MEIER.0, device, port, "SOCKET")
        )
        .await,
        not_owned
    );
    assert_eq!(
        ask(meier, &connect_query(MEIER.0, port, port, "INTERFACE")).await,
        not_owned
    );
    assert_eq!(
        ask(huber, &disconnect_query(HUBER.0, device)).await,
        not_owned
    );
}

/// `raw` with the rooms 9101 „Werkstatt“ and 9103 „Keller“ in the site of the device `device`,
/// and 9102 „Anderswo“ in a site of nobody.
fn with_rooms(mut raw: Value, device: u32) -> Value {
    let entry = raw["data"]["device_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["id"] == device.to_string().as_str())
        .expect("meier's device")
        .clone();
    let tenant = entry["location"]["tenant"].clone();
    let locations = raw["data"]["location_list"]
        .as_array_mut()
        .expect("locations");
    for (id, name, site) in [
        ("9101", "Werkstatt", entry["site"]["id"].clone()),
        ("9102", "Anderswo", json!("999999")),
        ("9103", "Keller", entry["site"]["id"].clone()),
    ] {
        locations.push(json!({
            "id": id, "name": name, "parent": null, "site": { "id": site }, "tenant": tenant,
        }));
    }
    raw
}

/// Meier puts its device in a room of its site, which changes no device's configuration; a
/// room of a site it has nothing in, and another's device, are refused.
#[tokio::test]
async fn a_device_goes_to_a_room_of_the_residents_sites() {
    let OwnedExample { raw, device, .. } = owned_example();
    let raw = with_rooms(raw, device);
    let query = |location: u32| {
        format!(
            r#"{{ changePlan(resident: {}, changes: [{{ deviceRoom: {{ device: {device}, location: {location} }} }}]) {{
                rooms {{ device {{ id }} location {{ id path }} }} devices {{ id }}
            }} }}"#,
            MEIER.0
        )
    };
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    let data = ask_on(raw.clone(), meier.clone(), &query(9101))
        .await
        .expect("a plan");
    assert_eq!(
        data["changePlan"]["rooms"],
        json!([{ "device": { "id": device }, "location": { "id": 9101, "path": ["Werkstatt"] } }])
    );
    assert_eq!(data["changePlan"]["devices"], json!([]));
    let not_owned = Err("\"NOT_OWNED\"".to_string());
    assert_eq!(ask_on(raw.clone(), meier, &query(9102)).await, not_owned);
    let huber = Role::Resident {
        owners: Box::new([HUBER]),
    };
    let query = query(9101).replace(
        &format!("resident: {}", MEIER.0),
        &format!("resident: {}", HUBER.0),
    );
    assert_eq!(ask_on(raw, huber, &query).await, not_owned);
}

/// `raw` with the owner of the interface `id` (and of nothing else there) set to Meier.
fn with_meiers_port(mut raw: Value, id: u32) -> Value {
    for interface in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .flat_map(|device| device["interfaces"].as_array_mut().into_iter().flatten())
        .filter(|interface| interface["id"] == id.to_string().as_str())
    {
        interface["owner"] = json!({ "id": MEIER.0.to_string(), "name": "Familie Meier" });
    }
    raw
}

/// A network for Meier's port at whose cable a device not Meier's hangs is refused, naming
/// that device.
#[tokio::test]
async fn a_port_with_anothers_device_takes_no_network() {
    let OwnedExample { raw, vlan, .. } = owned_example();
    let topology = topology_from(raw.clone());
    // a port of a device with RouterOS whose cable ends at a device without, the first by id
    let mut ports = topology
        .list_devices()
        .filter(|device| device.has_routeros())
        .flat_map(|device| device.interfaces().into_vec())
        .filter(|port| {
            port.connected_interfaces()
                .iter()
                .any(|far| far.device().is_some_and(|device| !device.has_routeros()))
                && port
                    .connected_interfaces()
                    .iter()
                    .all(|far| far.device().is_some_and(|device| !device.has_routeros()))
        })
        .map(|port| port.id().0)
        .collect::<Vec<_>>();
    ports.sort();
    let port = *ports.first().expect("a port to a device without RouterOS");
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    assert_eq!(
        ask_on(
            with_meiers_port(raw, port),
            meier,
            &port_query(MEIER.0, port, "INTERFACE", &vlan.to_string())
        )
        .await,
        Err("\"FOREIGN_DEVICE_AT_PORT\"".to_string())
    );
}

/// Meier's device can't be taken off its link while a device not Meier's hangs at another of
/// its ports: that one would go with it.
#[tokio::test]
async fn a_device_with_anothers_behind_it_stays_linked() {
    let OwnedExample { mut raw, .. } = owned_example();
    let topology = topology_from(raw.clone());
    // a RouterOS device with its Anschluss cabled and another device at another port, the
    // first by id
    let mut devices = topology
        .list_devices()
        .filter(|device| device.has_routeros())
        .filter(|device| {
            let Some(uplink) = uplink_port(device).filter(|port| port.cable().is_some()) else {
                return false;
            };
            device
                .interfaces()
                .iter()
                .filter(|port| port.id() != uplink.id())
                .flat_map(|port| port.connected_interfaces().into_vec())
                .any(|far| far.device().is_some_and(|far| far.id() != device.id()))
        })
        .map(|device| device.id().0)
        .collect::<Vec<_>>();
    devices.sort();
    let device = *devices.first().expect("a device with another behind it");
    for entry in raw["data"]["device_list"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let owner = match entry["id"] == device.to_string().as_str() {
            true => json!({ "id": MEIER.0.to_string(), "name": "Familie Meier" }),
            false => Value::Null,
        };
        entry["owner"] = owner;
    }
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    assert_eq!(
        ask_on(raw, meier, &disconnect_query(MEIER.0, device)).await,
        Err("\"FOREIGN_DEVICE_BEHIND\"".to_string())
    );
}

/// Meier's drafts saved together: a WLAN's name and its device in one room, then another.
/// Each change is planned on NetBox as the ones before leave it: the last one finds the
/// device in the room the one before put it in.
#[tokio::test]
async fn changes_are_planned_one_after_the_other() {
    let OwnedExample {
        raw, device, wlan, ..
    } = owned_example();
    let meier = Role::Resident {
        owners: Box::new([MEIER]),
    };
    let query = format!(
        r#"{{ changePlan(resident: {}, changes: [
            {{ wlan: {{ wlan: {wlan}, ssid: "meier-neu" }} }},
            {{ deviceRoom: {{ device: {device}, location: 9101 }} }},
            {{ deviceRoom: {{ device: {device}, location: 9103 }} }}
        ]) {{
            wlans {{ ssid }} rooms {{ location {{ id }} locationBefore {{ id }} }}
        }} }}"#,
        MEIER.0
    );
    let data = ask_on(with_rooms(raw, device), meier, &query)
        .await
        .expect("a plan");
    let plan = &data["changePlan"];
    assert_eq!(plan["wlans"], json!([{ "ssid": "meier-neu" }]));
    assert_eq!(
        plan["rooms"][1],
        json!({ "location": { "id": 9103 }, "locationBefore": { "id": 9101 } })
    );
}
