//! Writing a plan to NetBox (`write`) and taking it back (`undo`), each step an event of the
//! job (`NetboxWritten`, `NetboxUndone`).

use super::{
    CableEnd, Helper, PlanStep, Target, UpdateInterface, UpdatePort, UpdatePortVlan, UpdateWirelessLink,
    flags,
};
use crate::{
    Error,
    device_state::NetboxState,
    job::{Event, Log},
    netbox::rest::NetboxRest,
    topology::{
        PoeMode, RadioRole, Topology,
        findings::{InterfaceRef, IpVersion},
    },
};
use async_graphql::SimpleObject;
use serde_json::{Value, json};
use std::sync::Arc;

/// An object a helper wrote: created (`before` and `recreate` none), changed from `before`
/// (the JSON of the fields it changed) or deleted (`recreate`). `undo` takes it back from this.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct NetboxWritten {
    /// below NetBox' `api/`, `ipam/ip-addresses/12/`
    pub path: Box<str>,
    /// how NetBox names it (`display`)
    pub display: Box<str>,
    /// the JSON `undo` sets back
    #[graphql(skip)]
    pub before: Option<Box<str>>,
    /// what changed, for reading; empty for a created or deleted object
    pub changes: Vec<FieldChange>,
    pub deleted: bool,
    /// the JSON `undo` creates a deleted object again with (it gets a new id)
    #[graphql(skip)]
    pub recreate: Option<Box<str>>,
}

/// A field a helper changed, as NetBox shows its values.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FieldChange {
    pub field: Box<str>,
    pub value: Option<Box<str>>,
    pub previous: Option<Box<str>>,
}

/// An object `undo` took back: deleted, its fields set back or created again (`recreated`, at
/// `path` then, with its new id).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct NetboxUndone {
    pub path: Box<str>,
    pub display: Box<str>,
    pub deleted: bool,
    pub recreated: bool,
}

/// Writes `plan`, after computing it again from NetBox as it is now: it must still have
/// `fingerprint`. Each step checks NetBox once more right before writing.
pub async fn write(
    helper: &Helper,
    topology: &Arc<Topology>,
    fingerprint: &str,
    user: &str,
    log: &Log,
) -> Result<(), Error> {
    log.enter(NetboxState::Planning);
    let plan = helper.plan(topology).await?;
    if plan.fingerprint_value() != fingerprint {
        return Err(Error::PlanOutdated);
    }
    log.enter(NetboxState::Writing);
    let rest = NetboxRest::writer(format!("{} ({user})", plan.description))?;
    let mut created = Vec::<Option<u32>>::with_capacity(plan.steps.len());
    let resolve = |created: &[Option<u32>], target: Target| match target {
        Target::Existing(id) => Some(id),
        Target::Step(step) => created.get(step).copied().flatten(),
    };
    // the ids the plan's preview gives the objects it creates: step `n` has `u32::MAX - n`
    let first_created = u32::MAX - plan.steps.len() as u32;
    let is_created = |id: u32| id > first_created;
    for step in &plan.steps {
        log.check()?;
        // a device created by the plan has the interfaces of its device type already
        let on_new_device =
            matches!(step, PlanStep::CreateInterface(create) if is_created(create.device.id));
        let real = |id: u32| {
            if is_created(id) {
                created.get((u32::MAX - id) as usize).copied().flatten()
            } else {
                Some(id)
            }
        };
        let step = &step
            .with_real_ids(real)
            .ok_or_else(|| Error::NetboxChanged(plan.description.clone()))?;
        let id = match step {
            PlanStep::CreateDevice(create) => {
                let existing = rest.get(&format!("dcim/devices/?name={}", create.name)).await?;
                if existing["count"].as_u64() != Some(0) {
                    return Err(Error::NetboxChanged(
                        format!("{} exists already", create.name).into(),
                    ));
                }
                let mut body = json!({
                    "name": create.name,
                    "site": create.site_id,
                    "device_type": create.device_type_id,
                    "role": create.role_id,
                    "platform": create.platform_id,
                    "status": "active",
                });
                if let Some(tenant) = create.tenant_id {
                    body["tenant"] = json!(tenant);
                }
                if let Some(group) = create.wlan_group_id {
                    body["custom_fields"] = json!({ "wlan_group": group });
                }
                let object = rest.create("dcim/devices/", body).await?;
                Some(logged_create(log, "dcim/devices", &object)?)
            }
            PlanStep::CreateInterface(create) => {
                let existing = rest
                    .get(&format!(
                        "dcim/interfaces/?device_id={}&name={}",
                        create.device.id, create.name
                    ))
                    .await?;
                // from the device type: taken as it is, it goes with the device
                if on_new_device && let Some(id) = existing["results"][0]["id"].as_u64() {
                    created.push(Some(id as u32));
                    continue;
                }
                if existing["count"].as_u64() != Some(0) {
                    return Err(Error::NetboxChanged(
                        format!("{} has {} already", create.device.name, create.name).into(),
                    ));
                }
                let mut body =
                    json!({ "device": create.device.id, "name": create.name, "type": create.kind });
                if let Some(vlan) = create.vlan {
                    body["mode"] = json!("access");
                    body["untagged_vlan"] = json!(vlan);
                }
                if !create.tags.is_empty() {
                    body["tags"] = Value::Array(tag_ids(&rest, &create.tags).await?);
                }
                let object = rest.create("dcim/interfaces/", body).await?;
                let interface = logged_create(log, "dcim/interfaces", &object)?;
                // its own object, assigned to the interface, then its primary one; an undo
                // deletes it before the interface, which then has none again
                if let Some(mac) = &create.mac {
                    let mac = rest
                        .create(
                            "dcim/mac-addresses/",
                            json!({
                                "mac_address": mac,
                                "assigned_object_type": "dcim.interface",
                                "assigned_object_id": interface,
                            }),
                        )
                        .await?;
                    let mac_id = logged_create(log, "dcim/mac-addresses", &mac)?;
                    rest.update(
                        &format!("dcim/interfaces/{interface}/"),
                        json!({ "primary_mac_address": mac_id }),
                    )
                    .await?;
                }
                Some(interface)
            }
            PlanStep::CreateAddress(create) => {
                let interface = resolve(&created, create.on)
                    .ok_or_else(|| Error::NetboxChanged(create.interface.clone()))?;
                let host = create.address.split('/').next().unwrap_or_default();
                let existing = rest.get(&format!("ipam/ip-addresses/?address={host}")).await?;
                if existing["count"].as_u64() != Some(0) {
                    return Err(Error::NetboxChanged(
                        format!("{} is taken", create.address).into(),
                    ));
                }
                let object = rest
                    .create(
                        "ipam/ip-addresses/",
                        json!({
                            "address": create.address,
                            "status": "active",
                            "assigned_object_type": "dcim.interface",
                            "assigned_object_id": interface,
                        }),
                    )
                    .await?;
                Some(logged_create(log, "ipam/ip-addresses", &object)?)
            }
            PlanStep::SetPrimaryIp(set) => {
                let address = resolve(&created, set.address_id)
                    .ok_or_else(|| Error::NetboxChanged(set.address.clone()))?;
                let path = format!("dcim/devices/{}/", set.device.id);
                let field = match set.version {
                    IpVersion::V4 => "primary_ip4",
                    IpVersion::V6 => "primary_ip6",
                };
                let current = rest.get(&path).await?;
                let current_id = current[field]["id"].as_u64().map(|id| id as u32);
                if current_id != set.before_id {
                    return Err(Error::NetboxChanged(
                        format!("{}: {field}", set.device.name).into(),
                    ));
                }
                let before = json!({ field: current_id });
                let object = rest.update(&path, json!({ field: address })).await?;
                let shown = |object: &Value| object[field]["address"].as_str().map(Box::from);
                log.emit(Event::NetboxWritten(NetboxWritten {
                    path: path.into(),
                    display: display(&object),
                    before: Some(before.to_string().into()),
                    changes: vec![FieldChange {
                        field: field.into(),
                        value: shown(&object),
                        previous: shown(&current),
                    }],
                    deleted: false,
                    recreate: None,
                }));
                None
            }
            PlanStep::CreateCable(create) => {
                let ends = [(create.a_end, &create.a), (CableEnd::Interface, &create.b)];
                for (kind, end) in ends {
                    let current = rest.get(&format!("{}/{}/", kind.netbox().1, end.id)).await?;
                    if !current["cable"].is_null() {
                        return Err(Error::NetboxChanged(
                            format!("{} {} has a cable", end.device.name, end.name).into(),
                        ));
                    }
                }
                let end = |(kind, interface): (CableEnd, &InterfaceRef)| {
                    json!([{ "object_type": kind.netbox().0, "object_id": interface.id }])
                };
                let object = rest
                    .create(
                        "dcim/cables/",
                        json!({
                            "a_terminations": end(ends[0]),
                            "b_terminations": end(ends[1]),
                            "status": "connected",
                        }),
                    )
                    .await?;
                Some(logged_create(log, "dcim/cables", &object)?)
            }
            PlanStep::CreatePrefix(create) => {
                let prefixes = rest.get(&format!("ipam/prefixes/?prefix={}", create.prefix)).await?;
                let addresses = rest
                    .get(&format!("ipam/ip-addresses/?parent={}&limit=1", create.prefix))
                    .await?;
                if prefixes["count"].as_u64() != Some(0) || addresses["count"].as_u64() != Some(0)
                {
                    return Err(Error::NetboxChanged(
                        format!("{} is taken", create.prefix).into(),
                    ));
                }
                let mut body = json!({
                    "prefix": create.prefix,
                    "status": "active",
                    "description": create.description,
                });
                if let Some(site) = create.site {
                    body["scope_type"] = json!("dcim.site");
                    body["scope_id"] = json!(site);
                }
                if let Some(vlan) = create.vlan {
                    body["vlan"] = json!(vlan);
                }
                if let Some(seconds) = create.dhcp_lease_time {
                    body["custom_fields"] = json!({ "dhcp_lease_time": seconds });
                }
                let object = rest.create("ipam/prefixes/", body).await?;
                Some(logged_create(log, "ipam/prefixes", &object)?)
            }
            PlanStep::UpdateInterface(update) => {
                update_interface(&rest, update, log).await?;
                None
            }
            PlanStep::UpdatePort(update) => {
                let bridge = match update.bridge_id {
                    Some(target) => Some(
                        resolve(&created, target)
                            .ok_or_else(|| Error::NetboxChanged(update.interface.name.clone()))?,
                    ),
                    None => None,
                };
                update_port(&rest, update, bridge, log).await?;
                None
            }
            PlanStep::CreateWirelessLink(create) => {
                for end in [&create.a, &create.b] {
                    let current = rest.get(&format!("dcim/interfaces/{}/", end.id)).await?;
                    if !current["wireless_link"].is_null() {
                        return Err(Error::NetboxChanged(
                            format!("{} {} has a wireless link", end.device.name, end.name).into(),
                        ));
                    }
                }
                let object = rest
                    .create(
                        "wireless/wireless-links/",
                        json!({
                            "interface_a": create.a.id,
                            "interface_b": create.b.id,
                            "ssid": create.ssid,
                            "status": "connected",
                            "auth_type": "wpa-personal",
                            "auth_cipher": "aes",
                            "auth_psk": create.psk,
                        }),
                    )
                    .await?;
                Some(logged_create(log, "wireless/wireless-links", &object)?)
            }
            PlanStep::UpdateWirelessLink(update) => {
                update_wireless_link(&rest, update, log).await?;
                None
            }
            PlanStep::UpdatePortVlan(update) => {
                update_port_vlan(&rest, update, log).await?;
                None
            }
            PlanStep::DeleteInterface(delete) => {
                let interface = &delete.interface;
                let path = format!("dcim/interfaces/{}/", interface.id);
                let current = rest.get(&path).await?;
                let addresses = rest
                    .get(&format!("ipam/ip-addresses/?interface_id={}&limit=1", interface.id))
                    .await?;
                if current["name"].as_str() != Some(&*interface.name)
                    || current["device"]["id"].as_u64() != Some(interface.device.id.into())
                    || addresses["count"].as_u64() != Some(0)
                {
                    return Err(Error::NetboxChanged(
                        format!("{} {}", interface.device.name, interface.name).into(),
                    ));
                }
                let body = recreate(
                    &current,
                    &[
                        "device", "name", "label", "type", "enabled", "parent", "bridge", "mtu",
                        "mode", "untagged_vlan", "tagged_vlans", "vrf", "description", "tags",
                        "custom_fields",
                    ],
                );
                deleted(&rest, log, &path, &current, body).await?;
                None
            }
            PlanStep::DeleteWirelessLink(delete) => {
                let path = format!("wireless/wireless-links/{}/", delete.id);
                let current = rest.get(&path).await?;
                let ends = ["interface_a", "interface_b"].map(|end| current[end]["id"].as_u64());
                let same = |a: &InterfaceRef, b: &InterfaceRef| {
                    ends == [Some(a.id.into()), Some(b.id.into())]
                };
                if !same(&delete.a, &delete.b) && !same(&delete.b, &delete.a) {
                    return Err(Error::NetboxChanged(format!("wireless link {}", delete.id).into()));
                }
                // with its PSK, so that undo brings the same link back
                let body = recreate(
                    &current,
                    &[
                        "interface_a", "interface_b", "ssid", "status", "tenant", "auth_type",
                        "auth_cipher", "auth_psk", "distance", "distance_unit", "description",
                        "comments", "tags", "custom_fields",
                    ],
                );
                deleted(&rest, log, &path, &current, body).await?;
                None
            }
            PlanStep::DeleteCable(delete) => {
                let path = format!("dcim/cables/{}/", delete.id);
                let current = rest.get(&path).await?;
                let at_port = ["a_terminations", "b_terminations"].iter().any(|end| {
                    current[end].as_array().into_iter().flatten().any(|termination| {
                        termination["object_type"] == "dcim.interface"
                            && termination["object_id"].as_u64() == Some(delete.port.id.into())
                    })
                });
                if !at_port {
                    return Err(Error::NetboxChanged(
                        format!("{} {}: cable {}", delete.port.device.name, delete.port.name, delete.id)
                            .into(),
                    ));
                }
                deleted(&rest, log, &path, &current, recreate_cable(&current)).await?;
                None
            }
            PlanStep::DeleteAddress(delete) => {
                let path = format!("ipam/ip-addresses/{}/", delete.id);
                let current = rest.get(&path).await?;
                if current["address"].as_str() != Some(&*delete.address)
                    || current["assigned_object_id"].as_u64() != Some(delete.interface.id.into())
                {
                    return Err(Error::NetboxChanged(delete.address.clone()));
                }
                let device = rest
                    .get(&format!("dcim/devices/{}/", delete.interface.device.id))
                    .await?;
                if ["primary_ip4", "primary_ip6"]
                    .iter()
                    .any(|field| device[field]["id"].as_u64() == Some(delete.id.into()))
                {
                    return Err(Error::PrimaryAddress {
                        address: delete.address.clone(),
                        device: delete.interface.device.name.clone(),
                    });
                }
                let body = recreate(
                    &current,
                    &[
                        "address", "vrf", "tenant", "status", "role", "assigned_object_type",
                        "assigned_object_id", "dns_name", "description", "comments", "tags",
                        "custom_fields",
                    ],
                );
                deleted(&rest, log, &path, &current, body).await?;
                None
            }
            PlanStep::DeletePrefix(delete) => {
                let path = format!("ipam/prefixes/{}/", delete.id);
                let current = rest.get(&path).await?;
                let addresses = rest
                    .get(&format!("ipam/ip-addresses/?parent={}&limit=1", delete.prefix))
                    .await?;
                if current["prefix"].as_str() != Some(&*delete.prefix)
                    || addresses["count"].as_u64() != Some(0)
                {
                    return Err(Error::NetboxChanged(delete.prefix.clone()));
                }
                let body = recreate(
                    &current,
                    &[
                        "prefix", "vrf", "tenant", "vlan", "status", "role", "scope_type",
                        "scope_id", "is_pool", "mark_utilized", "description", "comments", "tags",
                        "custom_fields",
                    ],
                );
                deleted(&rest, log, &path, &current, body).await?;
                None
            }
        };
        created.push(id);
    }
    Ok(())
}

/// Takes back what a job of `write` wrote (its `NetboxWritten` events), the last first:
/// deletes what it created, sets back what it changed, creates again what it deleted. An object
/// created again gets a new id: what is taken back after it (an address of a VLAN interface
/// deleted after its addresses) refers to that one ([`Recreated`]).
pub async fn undo(
    job: u32,
    written: &[NetboxWritten],
    user: &str,
    log: &Log,
) -> Result<(), Error> {
    log.enter(NetboxState::Undoing);
    let rest = NetboxRest::writer(format!("Undo of provisioner job {job} ({user})"))?;
    let mut recreated = Recreated::default();
    for object in written.iter().rev() {
        log.check()?;
        let parse = |json: &str| {
            serde_json::from_str::<Value>(json).map_err(|error| Error::Preview(error.to_string().into()))
        };
        let undone = match (&object.recreate, &object.before) {
            (Some(body), _) => {
                // the collection the object was in
                let collection = object.path.trim_end_matches('/');
                let collection = &collection[..collection.rfind('/').map_or(0, |i| i + 1)];
                let mut body = parse(body)?;
                recreated.refer(&mut body);
                let created = rest.create(collection, body).await?;
                recreated.note(&object.path, &created);
                NetboxUndone {
                    path: format!("{collection}{}/", created["id"]).into(),
                    display: display(&created),
                    deleted: false,
                    recreated: true,
                }
            }
            (None, None) => {
                rest.delete(&object.path).await?;
                NetboxUndone {
                    path: object.path.clone(),
                    display: object.display.clone(),
                    deleted: true,
                    recreated: false,
                }
            }
            (None, Some(before)) => {
                let mut before = parse(before)?;
                recreated.refer(&mut before);
                rest.update(&object.path, before).await?;
                NetboxUndone {
                    path: object.path.clone(),
                    display: object.display.clone(),
                    deleted: false,
                    recreated: false,
                }
            }
        };
        log.emit(Event::NetboxUndone(undone));
    }
    Ok(())
}

/// The interfaces `undo` created again: their id before (in the paths and JSON it takes back)
/// and now.
#[derive(Debug, Default)]
struct Recreated {
    interfaces: std::collections::HashMap<u64, u64>,
}

impl Recreated {
    /// `created`, the object NetBox made again of the one at `path`.
    fn note(&mut self, path: &str, created: &Value) {
        let old = path
            .strip_prefix("dcim/interfaces/")
            .and_then(|id| id.trim_end_matches('/').parse().ok());
        if let (Some(old), Some(new)) = (old, created["id"].as_u64()) {
            self.interfaces.insert(old, new);
        }
    }

    /// `body` with its references to an interface created again by its new id: an address's
    /// assigned object, a wireless link's ends, a cable's terminations, a port's bridge or
    /// parent.
    fn refer(&self, body: &mut Value) {
        let new = |id: &mut Value| {
            if let Some(new) = id.as_u64().and_then(|old| self.interfaces.get(&old)) {
                *id = json!(new);
            }
        };
        if body["assigned_object_type"] == "dcim.interface" {
            new(&mut body["assigned_object_id"]);
        }
        for field in ["interface_a", "interface_b", "bridge", "parent"] {
            if let Some(id) = body.get_mut(field) {
                new(id);
            }
        }
        for end in ["a_terminations", "b_terminations"] {
            for termination in body.get_mut(end).and_then(Value::as_array_mut).into_iter().flatten() {
                if termination["object_type"] == "dcim.interface" {
                    new(&mut termination["object_id"]);
                }
            }
        }
    }
}

/// Writes `update` once the interface is still as the plan found it.
async fn update_interface(
    rest: &NetboxRest,
    update: &UpdateInterface,
    log: &Log,
) -> Result<(), Error> {
    let name = &update.interface;
    let path = format!("dcim/interfaces/{}/", name.id);
    let current = rest.get(&path).await?;
    let label = current["label"].as_str().filter(|l| !l.is_empty());
    let mtu = current["mtu"].as_u64().map(|mtu| mtu as u16);
    let tags = current["tags"].as_array().cloned().unwrap_or_default();
    let has = |tag: &flags::InterfaceTag| tags.iter().any(|t| t["slug"] == tag.slug());
    let poe = match current["poe_mode"]["value"].as_str() {
        Some("pse") => Some(PoeMode::Pse),
        Some("pd") => Some(PoeMode::Pd),
        _ => None,
    };
    if label != update.label_before.as_deref()
        || mtu != update.mtu_before
        || ((update.poe.is_some() || update.poe_removed) && poe != update.poe_before)
        || update.tags_added.iter().any(has)
        || !update.tags_removed.iter().all(has)
    {
        return Err(Error::NetboxChanged(
            format!("{} {}", name.device.name, name.name).into(),
        ));
    }
    let mut change = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    let mut changes = Vec::new();
    if let Some(new) = &update.label {
        change.insert("label".into(), json!(new));
        before.insert("label".into(), json!(label.unwrap_or_default()));
        changes.push(FieldChange {
            field: "label".into(),
            value: Some(new.clone()).filter(|new| !new.is_empty()),
            previous: label.map(Box::from),
        });
    }
    if let Some(new) = update.mtu {
        change.insert("mtu".into(), json!(new));
        before.insert("mtu".into(), json!(mtu));
        changes.push(FieldChange {
            field: "mtu".into(),
            value: Some(new.to_string().into()),
            previous: mtu.map(|mtu| mtu.to_string().into()),
        });
    }
    if let Some(new) = update.poe {
        change.insert("poe_mode".into(), json!(new.value()));
        before.insert("poe_mode".into(), json!(poe.map(PoeMode::value).unwrap_or_default()));
        changes.push(FieldChange {
            field: "poe_mode".into(),
            value: Some(new.value().into()),
            previous: poe.map(|poe| poe.value().into()),
        });
    }
    if update.poe_removed {
        change.insert("poe_mode".into(), json!(""));
        before.insert("poe_mode".into(), json!(poe.map(PoeMode::value).unwrap_or_default()));
        changes.push(FieldChange {
            field: "poe_mode".into(),
            value: None,
            previous: poe.map(|poe| poe.value().into()),
        });
        // NetBox refuses a PoE type without a mode ("Must specify PoE mode when designating
        // a PoE type"), so it goes too; undoing sets both again
        if let Some(poe_type) = current["poe_type"]["value"].as_str().filter(|t| !t.is_empty()) {
            change.insert("poe_type".into(), json!(""));
            before.insert("poe_type".into(), json!(poe_type));
            changes.push(FieldChange {
                field: "poe_type".into(),
                value: None,
                previous: Some(poe_type.into()),
            });
        }
    }
    if !update.tags_added.is_empty() || !update.tags_removed.is_empty() {
        let mut ids: Vec<Value> = tags
            .iter()
            .filter(|t| !update.tags_removed.iter().any(|tag| t["slug"] == tag.slug()))
            .map(|t| t["id"].clone())
            .collect();
        ids.extend(tag_ids(rest, &update.tags_added).await?);
        let slugs = |list: &[flags::InterfaceTag]| {
            let text = list.iter().map(|tag| tag.slug()).collect::<Vec<_>>().join(", ");
            (!text.is_empty()).then(|| Box::from(text))
        };
        change.insert("tags".into(), Value::Array(ids));
        before.insert(
            "tags".into(),
            Value::Array(tags.iter().map(|t| t["id"].clone()).collect()),
        );
        changes.push(FieldChange {
            field: "tags".into(),
            value: slugs(&update.tags_added),
            previous: slugs(&update.tags_removed),
        });
    }
    let object = rest.update(&path, Value::Object(change)).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(&object),
        before: Some(Value::Object(before).to_string().into()),
        changes,
        deleted: false,
        recreate: None,
    }));
    Ok(())
}

/// Writes `update` once the port still has the VLANs the plan found.
async fn update_port_vlan(
    rest: &NetboxRest,
    update: &UpdatePortVlan,
    log: &Log,
) -> Result<(), Error> {
    let port = &update.interface;
    let path = format!("dcim/interfaces/{}/", port.id);
    let current = rest.get(&path).await?;
    let untagged = current["untagged_vlan"]["id"].as_u64();
    let mut tagged = current["tagged_vlans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|vlan| vlan["id"].as_u64())
        .collect::<Vec<_>>();
    tagged.sort_unstable();
    let mut tagged_before = update.tagged_before.iter().map(|v| u64::from(v.id)).collect::<Vec<_>>();
    tagged_before.sort_unstable();
    if untagged != update.vlan_before.as_ref().map(|v| v.id.into()) || tagged != tagged_before {
        return Err(Error::NetboxChanged(format!("{} {}", port.device.name, port.name).into()));
    }
    let change = match &update.vlan {
        Some(vlan) => json!({ "mode": "access", "untagged_vlan": vlan.id, "tagged_vlans": [] }),
        None => json!({ "mode": null, "untagged_vlan": null, "tagged_vlans": [] }),
    };
    let before = json!({
        "mode": current["mode"]["value"].clone(),
        "untagged_vlan": untagged,
        "tagged_vlans": tagged,
    });
    let name = |vlan: &crate::topology::findings::VlanRef| -> Box<str> {
        format!("{} ({})", vlan.vid, vlan.name).into()
    };
    let mut changes = vec![FieldChange {
        field: "untagged_vlan".into(),
        value: update.vlan.as_ref().map(name),
        previous: update.vlan_before.as_ref().map(name),
    }];
    if !update.tagged_before.is_empty() {
        changes.push(FieldChange {
            field: "tagged_vlans".into(),
            value: None,
            previous: Some(
                update.tagged_before.iter().map(name).collect::<Vec<_>>().join(", ").into(),
            ),
        });
    }
    let object = rest.update(&path, change).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(&object),
        before: Some(before.to_string().into()),
        changes,
        deleted: false,
        recreate: None,
    }));
    Ok(())
}

/// Writes `update` once the port is still as the plan found it; `bridge` is its new bridge's id.
async fn update_port(
    rest: &NetboxRest,
    update: &UpdatePort,
    bridge: Option<u32>,
    log: &Log,
) -> Result<(), Error> {
    let port = &update.interface;
    let path = format!("dcim/interfaces/{}/", port.id);
    let current = rest.get(&path).await?;
    let name = current["name"].as_str().unwrap_or_default();
    let role = match current["rf_role"]["value"].as_str() {
        Some("ap") => Some(RadioRole::Ap),
        Some("station") => Some(RadioRole::Station),
        _ => None,
    };
    let bridge_before = current["bridge"]["id"].as_u64().map(|id| id as u32);
    if name != &*port.name
        || (update.rf_role.is_some() && role != update.rf_role_before)
        || ((bridge.is_some() || update.leaves_bridge.is_some())
            && bridge_before != update.bridge_before)
    {
        return Err(Error::NetboxChanged(format!("{} {}", port.device.name, port.name).into()));
    }
    let mut change = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    let mut changes = Vec::new();
    if let Some(new) = &update.name {
        change.insert("name".into(), json!(new));
        before.insert("name".into(), json!(name));
        changes.push(FieldChange {
            field: "name".into(),
            value: Some(new.clone()),
            previous: Some(name.into()),
        });
    }
    if let Some(new) = update.rf_role {
        change.insert("rf_role".into(), json!(new.value()));
        before.insert("rf_role".into(), json!(role.map(RadioRole::value).unwrap_or_default()));
        changes.push(FieldChange {
            field: "rf_role".into(),
            value: Some(new.value().into()),
            previous: role.map(|role| role.value().into()),
        });
    }
    if let Some(new) = bridge {
        change.insert("bridge".into(), json!(new));
        before.insert("bridge".into(), json!(bridge_before));
        changes.push(FieldChange {
            field: "bridge".into(),
            value: update.bridge.clone(),
            previous: current["bridge"]["name"].as_str().map(Box::from),
        });
    } else if update.leaves_bridge.is_some() {
        change.insert("bridge".into(), Value::Null);
        before.insert("bridge".into(), json!(bridge_before));
        changes.push(FieldChange {
            field: "bridge".into(),
            value: None,
            previous: current["bridge"]["name"].as_str().map(Box::from),
        });
    }
    let object = rest.update(&path, Value::Object(change)).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(&object),
        before: Some(Value::Object(before).to_string().into()),
        changes,
        deleted: false,
        recreate: None,
    }));
    Ok(())
}

/// Writes `update` once the link still has its SSID and, where a PSK comes, none.
async fn update_wireless_link(
    rest: &NetboxRest,
    update: &UpdateWirelessLink,
    log: &Log,
) -> Result<(), Error> {
    let path = format!("wireless/wireless-links/{}/", update.id);
    let current = rest.get(&path).await?;
    let ssid = current["ssid"].as_str().unwrap_or_default();
    let psk = current["auth_psk"].as_str().unwrap_or_default();
    if ssid != &*update.ssid_before || (update.psk.is_some() && !psk.is_empty()) {
        return Err(Error::NetboxChanged(format!("wireless link {}", update.id).into()));
    }
    let mut change = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    let mut changes = Vec::new();
    if let Some(new) = &update.ssid {
        change.insert("ssid".into(), json!(new));
        before.insert("ssid".into(), json!(ssid));
        changes.push(FieldChange {
            field: "ssid".into(),
            value: Some(new.clone()),
            previous: (!ssid.is_empty()).then(|| ssid.into()),
        });
    }
    if let Some(new) = &update.psk {
        let auth = |field: &str| current[field]["value"].as_str().unwrap_or_default().to_string();
        change.insert("auth_type".into(), json!("wpa-personal"));
        change.insert("auth_cipher".into(), json!("aes"));
        change.insert("auth_psk".into(), json!(new));
        before.insert("auth_type".into(), json!(auth("auth_type")));
        before.insert("auth_cipher".into(), json!(auth("auth_cipher")));
        before.insert("auth_psk".into(), json!(""));
        // the key itself stays out of the job's log
        changes.push(FieldChange {
            field: "auth_psk".into(),
            value: Some("•••".into()),
            previous: None,
        });
    }
    let object = rest.update(&path, Value::Object(change)).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(&object),
        before: Some(Value::Object(before).to_string().into()),
        changes,
        deleted: false,
        recreate: None,
    }));
    Ok(())
}

/// The ids of `tags` in NetBox.
async fn tag_ids(rest: &NetboxRest, tags: &[flags::InterfaceTag]) -> Result<Vec<Value>, Error> {
    let mut ids = Vec::new();
    for tag in tags {
        let found = rest.get(&format!("extras/tags/?slug={}", tag.slug())).await?;
        let id = found["results"][0]["id"].clone();
        if id.is_null() {
            return Err(Error::TagMissing(tag.slug().into()));
        }
        ids.push(id);
    }
    Ok(ids)
}

/// Deletes the object at `path` (`current` as NetBox has it), `undo` creates it again from
/// `body`.
async fn deleted(
    rest: &NetboxRest,
    log: &Log,
    path: &str,
    current: &Value,
    body: Value,
) -> Result<(), Error> {
    rest.delete(path).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(current),
        before: None,
        changes: Vec::new(),
        deleted: true,
        recreate: Some(body.to_string().into()),
    }));
    Ok(())
}

/// The `fields` of `object` (as NetBox returns it) as NetBox takes them to create it: a choice
/// by its value, a related object by its id, tags by their ids.
fn recreate(object: &Value, fields: &[&str]) -> Value {
    let writable = |value: &Value| match value {
        Value::Object(map) if map.contains_key("value") => map["value"].clone(),
        Value::Object(map) if map.contains_key("id") => map["id"].clone(),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| item.get("id").cloned().unwrap_or_else(|| item.clone()))
                .collect(),
        ),
        other => other.clone(),
    };
    Value::Object(
        fields
            .iter()
            .filter(|field| !object[**field].is_null())
            .map(|field| (field.to_string(), writable(&object[*field])))
            .collect(),
    )
}

/// A cable as NetBox takes it to create it: its terminations by type and id.
fn recreate_cable(cable: &Value) -> Value {
    let mut body = recreate(
        cable,
        &[
            "type", "status", "tenant", "label", "color", "length", "length_unit", "description",
            "comments", "tags", "custom_fields",
        ],
    );
    for end in ["a_terminations", "b_terminations"] {
        let terminations: Vec<Value> = cable[end]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| json!({ "object_type": t["object_type"], "object_id": t["object_id"] }))
            .collect();
        body[end] = Value::Array(terminations);
    }
    body
}

fn display(object: &Value) -> Box<str> {
    object["display"].as_str().unwrap_or_default().into()
}

/// Logs the object NetBox created below `path` and returns its id.
fn logged_create(log: &Log, path: &str, object: &Value) -> Result<u32, Error> {
    let id = object["id"]
        .as_u64()
        .ok_or_else(|| Error::Preview(format!("no id from {path}").into()))? as u32;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: format!("{path}/{id}/").into(),
        display: display(object),
        before: None,
        changes: Vec::new(),
        deleted: false,
        recreate: None,
    }));
    Ok(id)
}

#[cfg(test)]
mod test {
    use super::*;

    /// The address of a VLAN interface deleted after it refers to the interface created again,
    /// a cable to another interface stays as it was.
    #[test]
    fn taken_back_objects_refer_to_the_interface_created_again() {
        let mut recreated = Recreated::default();
        recreated.note("dcim/interfaces/38/", &json!({ "id": 66 }));
        let mut address = json!({
            "address": "192.168.105.1/24",
            "assigned_object_type": "dcim.interface",
            "assigned_object_id": 38,
        });
        recreated.refer(&mut address);
        assert_eq!(address["assigned_object_id"], 66);
        let mut cable = json!({
            "a_terminations": [{ "object_type": "dcim.interface", "object_id": 38 }],
            "b_terminations": [{ "object_type": "dcim.interface", "object_id": 7 }],
        });
        recreated.refer(&mut cable);
        assert_eq!(cable["a_terminations"][0]["object_id"], 66);
        assert_eq!(cable["b_terminations"][0]["object_id"], 7);
        let mut prefix = json!({ "prefix": "192.168.105.0/24", "vlan": 38 });
        recreated.refer(&mut prefix);
        assert_eq!(prefix["vlan"], 38, "not an interface");
    }
}
