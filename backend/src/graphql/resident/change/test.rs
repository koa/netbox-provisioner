//! Scope: Tests of `graphql::resident::change`; nothing else.
use crate::{
    graphql::resident::create_resident_schema,
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
        r#"{{ changePlan(resident: {resident}, change: {{ wlan: {{ wlan: {wlan}, ssid: "{ssid}" }} }}) {{
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
        r#"{{ changePlan(resident: {resident}, change: {{ portVlan: {{ port: {port}, kind: {kind}, vlan: {vlan} }} }}) {{
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
        r#"{{ changePlan(resident: {resident}, change: {{ connectDevice: {{ device: {device}, port: {port}, kind: {kind} }} }}) {{
            fingerprint devices {{ name }}
        }} }}"#
    )
}

fn disconnect_query(resident: u32, device: u32) -> String {
    format!(
        r#"{{ changePlan(resident: {resident}, change: {{ disconnectDevice: {device} }}) {{
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
