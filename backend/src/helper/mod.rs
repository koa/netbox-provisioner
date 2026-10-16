//! The helpers of `docs/concepts/config-helper.md`: for a standard case, a plan of the NetBox
//! objects to create or change, shown with its effect (`NetboxPlan.preview`) before it is
//! written. They write to NetBox only, never to a device; the device gets it by applying its
//! config as always.
//!
//! A plan is computed again when it is written (`write`): it must be the one shown
//! (`fingerprint`), and each step checks that NetBox is still as the plan found it, so a
//! change made in between stops it instead of being overwritten. Every write is an event of
//! the job (`NetboxWritten`), which is what `undo` takes back.
pub mod connection;
pub mod loopback;
pub mod routes;

use crate::{
    Error,
    device::AccessibleDevice,
    job::{Event, Log},
    netbox::rest::NetboxRest,
    topology::{
        DeviceId, Topology,
        access::{AdhocCredentials, device::DeviceAccess},
        findings::{DeviceRef, InterfaceRef, IpVersion},
    },
};
use async_graphql::{ComplexObject, OneofObject, SimpleObject, Union};
use serde_json::{Value, json};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

/// Which helper computes a plan, and for what: computed again when it is written.
#[derive(Debug, Clone, PartialEq, Eq, OneofObject)]
pub enum Helper {
    /// `lo` with an address of each loopback pool, as primary IPs, for the device of this id
    Loopback(u32),
    /// a direct link between two routers, for the device of `interface`
    Connection(connection::ConnectionInput),
}

impl Helper {
    pub async fn plan(&self, topology: &Arc<Topology>) -> Result<NetboxPlan, Error> {
        match self {
            Helper::Loopback(_) => loopback::plan(&self.device(topology)?).await,
            Helper::Connection(input) => connection::plan(topology, input).await,
        }
    }
    /// The device the plan is for, whose job writes it.
    pub fn device(&self, topology: &Arc<Topology>) -> Result<DeviceAccess, Error> {
        match self {
            Helper::Loopback(id) => topology
                .get_device_by_id(&DeviceId(*id))
                .ok_or(Error::UnknownDevice(*id)),
            Helper::Connection(input) => topology
                .get_interface_by_id(crate::topology::InterfaceId(input.interface))
                .and_then(|interface| interface.device())
                .ok_or(Error::UnknownInterface(input.interface)),
        }
    }
}

/// What a helper writes to NetBox for `device`, in this order: each step leaves NetBox valid.
#[derive(Clone, SimpleObject)]
#[graphql(complex)]
pub struct NetboxPlan {
    pub steps: Vec<PlanStep>,
    /// what the changelog of NetBox says, with the user
    pub description: Box<str>,
    /// the router whose routes the new addresses were checked against
    pub checked_with: Option<Box<str>>,
    /// addresses NetBox has free that the network routes: passed over, to be documented
    pub undocumented: Vec<routes::UndocumentedAddress>,
    #[graphql(skip)]
    device: DeviceAccess,
}

#[ComplexObject]
impl NetboxPlan {
    /// Identifies the plan: writing refuses a plan that has become another one by then.
    async fn fingerprint(&self) -> String {
        self.fingerprint_value()
    }
    /// The device as NetBox has it after the plan: its findings, and over `access` the script
    /// applying would bring then. Its management address may be a new one the device doesn't
    /// have yet: `access(target:)` with the address it has now reaches it.
    async fn preview(&self) -> Result<DeviceAccess, crate::error_info::ApiError> {
        Ok(self.preview_device()?)
    }
    /// The device reached as now (`target`, credentials as for `DeviceAccess.access`), with its
    /// config generated from NetBox after the plan: `generateCfg` is what applying would do
    /// then.
    async fn preview_access(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Option<AccessibleDevice>, crate::error_info::ApiError> {
        let preview = self.preview_device()?;
        Ok(self
            .device
            .connect(target, credential_name, adhoc_credentials)
            .await?
            .map(|access| access.generating_for(preview)))
    }
}

impl NetboxPlan {
    pub fn new(device: &DeviceAccess, description: impl Into<Box<str>>) -> Self {
        Self {
            steps: Vec::new(),
            description: description.into(),
            checked_with: None,
            undocumented: Vec::new(),
            device: device.clone(),
        }
    }
    pub fn device(&self) -> &DeviceAccess {
        &self.device
    }
    pub fn fingerprint_value(&self) -> String {
        let mut hasher = DefaultHasher::new();
        format!("{:?}", self.steps).hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
    /// The NetBox topology after the plan (`Topology::rebuilt_with`).
    pub fn preview_topology(&self) -> Result<Arc<Topology>, Error> {
        let topology = self
            .device
            .topology()
            .rebuilt_with(|json| self.apply_to(json))
            .ok_or(Error::NoPreview)?
            .map_err(|error| Error::Preview(error.to_string().into()))?;
        Ok(Arc::new(topology))
    }
    fn preview_device(&self) -> Result<DeviceAccess, Error> {
        self.preview_topology()?
            .get_device_by_id(&self.device.id())
            .ok_or(Error::UnknownDevice(self.device.id().0))
    }

    /// The plan on the JSON of NetBox' `ResponseData`, for the preview. Created objects get ids
    /// from the top of the range, which NetBox doesn't hand out.
    fn apply_to(&self, json: &mut Value) {
        let created_id = |step: usize| (u32::MAX - step as u32).to_string();
        let target_id = |target: Target| match target {
            Target::Existing(id) => id.to_string(),
            Target::Step(step) => created_id(step),
        };
        fn device(json: &mut Value, id: u32) -> Option<&mut Value> {
            let id = id.to_string();
            json["device_list"]
                .as_array_mut()?
                .iter_mut()
                .find(|device| device["id"] == id.as_str())
        }
        fn interface<'j>(json: &'j mut Value, id: &str) -> Option<&'j mut Value> {
            json["device_list"]
                .as_array_mut()?
                .iter_mut()
                .filter_map(|device| device["interfaces"].as_array_mut())
                .flatten()
                .find(|interface| interface["id"] == id)
        }
        for (index, step) in self.steps.iter().enumerate() {
            match step {
                PlanStep::CreateInterface(create) => {
                    if let Some(interfaces) = device(json, create.device.id)
                        .and_then(|device| device["interfaces"].as_array_mut())
                    {
                        interfaces.push(json!({
                            "id": created_id(index),
                            "name": create.name,
                            "label": "",
                            "ip_addresses": [],
                            "enabled": true,
                            "type": create.kind,
                            "mtu": null,
                            "bridge": null,
                            "tags": [],
                            "untagged_vlan": null,
                            "tagged_vlans": [],
                            "poe_mode": null,
                            "rf_role": null,
                            "rf_channel_frequency": null,
                            "rf_channel_width": null,
                            "tx_power": null,
                            "wireless_link": null,
                        }));
                    }
                }
                PlanStep::CreateAddress(create) => {
                    if let Some(addresses) = interface(json, &target_id(create.on))
                        .and_then(|interface| interface["ip_addresses"].as_array_mut())
                    {
                        addresses.push(json!({ "id": created_id(index) }));
                    }
                    if let Some(addresses) = json["ip_address_list"].as_array_mut() {
                        addresses.push(json!({
                            "id": created_id(index),
                            "address": create.address,
                        }));
                    }
                }
                PlanStep::SetPrimaryIp(set) => {
                    let field = match set.version {
                        IpVersion::V4 => "primary_ip4",
                        IpVersion::V6 => "primary_ip6",
                    };
                    if let Some(device) = device(json, set.device.id) {
                        device[field] = json!({ "id": target_id(set.address_id) });
                    }
                }
                PlanStep::CreateCable(create) => {
                    let end = |interface: &InterfaceRef| {
                        json!([{ "__typename": "InterfaceType", "id": interface.id.to_string() }])
                    };
                    if let Some(cables) = json["cable_list"].as_array_mut() {
                        cables.push(json!({
                            "id": created_id(index),
                            "a_terminations": end(&create.a),
                            "b_terminations": end(&create.b),
                        }));
                    }
                }
                PlanStep::CreatePrefix(create) => {
                    if let Some(prefixes) = json["prefix_list"].as_array_mut() {
                        prefixes.push(json!({
                            "id": created_id(index),
                            "prefix": create.prefix,
                            "description": create.description,
                            "tags": [],
                            "scope": null,
                        }));
                    }
                }
                PlanStep::UpdateInterface(update) => {
                    if let Some(interface) = interface(json, &update.interface.id.to_string()) {
                        if let Some(label) = &update.label {
                            interface["label"] = json!(label);
                        }
                        if let Some(mtu) = update.mtu {
                            interface["mtu"] = json!(mtu);
                        }
                        if update.ospf
                            && let Some(tags) = interface["tags"].as_array_mut()
                        {
                            tags.push(json!({ "slug": "ospf" }));
                        }
                    }
                }
            }
        }
    }
}

/// One object a plan creates or changes.
#[derive(Debug, Clone, PartialEq, Eq, Union)]
pub enum PlanStep {
    CreateInterface(CreateInterface),
    CreateAddress(CreateAddress),
    SetPrimaryIp(SetPrimaryIp),
    CreateCable(CreateCable),
    CreatePrefix(CreatePrefix),
    UpdateInterface(UpdateInterface),
}

/// An object of NetBox or one an earlier step of the plan creates.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Target {
    Existing(u32),
    Step(usize),
}

/// A new interface `name` of NetBox type `kind` (`virtual`) on `device`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateInterface {
    pub device: DeviceRef,
    pub name: Box<str>,
    pub kind: Box<str>,
}

/// A new IP address on `interface` (its name; it may be one the plan creates), the next free
/// one of `prefix` when the plan was computed: NetBox may have handed it out by the time the
/// plan is written, then writing stops.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateAddress {
    /// with its length, `192.168.100.9/32`
    pub address: Box<str>,
    pub device: DeviceRef,
    pub interface: Box<str>,
    /// the prefix it is taken from: the pool's, or the one the plan creates
    pub prefix: Box<str>,
    #[graphql(skip)]
    pub on: Target,
}

/// `address` becomes the primary IP of `device` for `version`, instead of `before`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct SetPrimaryIp {
    pub device: DeviceRef,
    pub version: IpVersion,
    pub address: Box<str>,
    pub before: Option<Box<str>>,
    #[graphql(skip)]
    pub address_id: Target,
    #[graphql(skip)]
    pub before_id: Option<u32>,
}

/// A new cable between two interfaces.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateCable {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

/// A new prefix, the next free one of the pool prefix `pool` when the plan was computed (as for
/// `CreateAddress`). It carries no tag: a pool's prefixes have one.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreatePrefix {
    /// `172.17.1.28/31`
    pub prefix: Box<str>,
    pub pool: Box<str>,
    pub description: Box<str>,
}

/// Changes of an interface: a new label, a new MTU (each none to keep it) and the tag `ospf`
/// added.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UpdateInterface {
    pub interface: InterfaceRef,
    pub label: Option<Box<str>>,
    pub label_before: Option<Box<str>>,
    pub mtu: Option<u16>,
    pub mtu_before: Option<u16>,
    pub ospf: bool,
}

/// An object a helper wrote: created (`before` is none) or changed from `before` (the JSON
/// of the fields it changed). `undo` takes it back from this.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct NetboxWritten {
    /// below NetBox' `api/`, `ipam/ip-addresses/12/`
    pub path: Box<str>,
    /// how NetBox names it (`display`)
    pub display: Box<str>,
    /// the JSON `undo` sets back
    #[graphql(skip)]
    pub before: Option<Box<str>>,
    /// what changed, for reading; empty for a created object
    pub changes: Vec<FieldChange>,
}

/// A field a helper changed, as NetBox shows its values.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct FieldChange {
    pub field: Box<str>,
    pub value: Option<Box<str>>,
    pub previous: Option<Box<str>>,
}

/// An object `undo` took back: deleted or its fields set back.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct NetboxUndone {
    pub path: Box<str>,
    pub display: Box<str>,
    pub deleted: bool,
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
    let plan = helper.plan(topology).await?;
    if plan.fingerprint_value() != fingerprint {
        return Err(Error::PlanOutdated);
    }
    let rest = NetboxRest::writer(format!("{} ({user})", plan.description))?;
    let mut created = Vec::<Option<u32>>::with_capacity(plan.steps.len());
    let resolve = |created: &[Option<u32>], target: Target| match target {
        Target::Existing(id) => Some(id),
        Target::Step(step) => created.get(step).copied().flatten(),
    };
    for step in &plan.steps {
        log.check()?;
        let id = match step {
            PlanStep::CreateInterface(create) => {
                let existing = rest
                    .get(&format!(
                        "dcim/interfaces/?device_id={}&name={}",
                        create.device.id, create.name
                    ))
                    .await?;
                if existing["count"].as_u64() != Some(0) {
                    return Err(Error::NetboxChanged(
                        format!("{} has {} already", create.device.name, create.name).into(),
                    ));
                }
                let object = rest
                    .create(
                        "dcim/interfaces/",
                        json!({ "device": create.device.id, "name": create.name, "type": create.kind }),
                    )
                    .await?;
                Some(logged_create(log, "dcim/interfaces", &object)?)
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
                }));
                None
            }
            PlanStep::CreateCable(create) => {
                for end in [&create.a, &create.b] {
                    let current = rest.get(&format!("dcim/interfaces/{}/", end.id)).await?;
                    if !current["cable"].is_null() {
                        return Err(Error::NetboxChanged(
                            format!("{} {} has a cable", end.device.name, end.name).into(),
                        ));
                    }
                }
                let end = |interface: &InterfaceRef| {
                    json!([{ "object_type": "dcim.interface", "object_id": interface.id }])
                };
                let object = rest
                    .create(
                        "dcim/cables/",
                        json!({
                            "a_terminations": end(&create.a),
                            "b_terminations": end(&create.b),
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
                let object = rest
                    .create(
                        "ipam/prefixes/",
                        json!({
                            "prefix": create.prefix,
                            "status": "active",
                            "description": create.description,
                        }),
                    )
                    .await?;
                Some(logged_create(log, "ipam/prefixes", &object)?)
            }
            PlanStep::UpdateInterface(update) => {
                update_interface(&rest, update, log).await?;
                None
            }
        };
        created.push(id);
    }
    Ok(())
}

/// Takes back what a job of `write` wrote (its `NetboxWritten` events), the last first:
/// deletes what it created, sets back what it changed.
pub async fn undo(
    job: u32,
    written: &[NetboxWritten],
    user: &str,
    log: &Log,
) -> Result<(), Error> {
    let rest = NetboxRest::writer(format!("Undo of provisioner job {job} ({user})"))?;
    for object in written.iter().rev() {
        log.check()?;
        match &object.before {
            None => rest.delete(&object.path).await?,
            Some(before) => {
                let before: Value = serde_json::from_str(before)
                    .map_err(|error| Error::Preview(error.to_string().into()))?;
                rest.update(&object.path, before).await?;
            }
        }
        log.emit(Event::NetboxUndone(NetboxUndone {
            path: object.path.clone(),
            display: object.display.clone(),
            deleted: object.before.is_none(),
        }));
    }
    Ok(())
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
    let has_ospf = tags.iter().any(|tag| tag["slug"] == "ospf");
    if label != update.label_before.as_deref()
        || mtu != update.mtu_before
        || (update.ospf && has_ospf)
    {
        return Err(Error::NetboxChanged(
            format!("{} {}", name.device.name, name.name).into(),
        ));
    }
    let tag_ids = tags.iter().map(|tag| tag["id"].clone()).collect::<Vec<_>>();
    let mut change = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    let mut changes = Vec::new();
    if let Some(new) = &update.label {
        change.insert("label".into(), json!(new));
        before.insert("label".into(), json!(label.unwrap_or_default()));
        changes.push(FieldChange {
            field: "label".into(),
            value: Some(new.clone()),
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
    if update.ospf {
        let found = rest.get("extras/tags/?slug=ospf").await?;
        let ospf = found["results"][0]["id"].clone();
        if ospf.is_null() {
            return Err(Error::TagMissing("ospf".into()));
        }
        let mut with_ospf = tag_ids.clone();
        with_ospf.push(ospf);
        change.insert("tags".into(), Value::Array(with_ospf));
        before.insert("tags".into(), Value::Array(tag_ids));
        changes.push(FieldChange {
            field: "tags".into(),
            value: Some("ospf".into()),
            previous: None,
        });
    }
    let object = rest.update(&path, Value::Object(change)).await?;
    log.emit(Event::NetboxWritten(NetboxWritten {
        path: path.into(),
        display: display(&object),
        before: Some(Value::Object(before).to_string().into()),
        changes,
    }));
    Ok(())
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
    }));
    Ok(id)
}

/// The device, as `DeviceRef`, for a plan step.
pub(crate) fn device_ref(device: &DeviceAccess) -> DeviceRef {
    crate::topology::findings::device_ref(device)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        device::ros::netbox_example_test::example_topology,
        topology::{
            access::{AccessTopology, interface::InterfaceAccess},
            findings::interface_ref,
        },
    };

    /// The preview is NetBox after the plan: a created interface with its new address, which
    /// is the primary IP; the topology the plan is computed from stays as it was.
    #[test]
    fn preview_has_what_the_plan_creates() {
        let topology = example_topology();
        let device = topology
            .list_devices()
            .find(|d| d.has_routeros() && d.primary_ip_v4().is_some())
            .expect("a router");
        let mut plan = NetboxPlan::new(&device, "test");
        plan.steps = vec![
            PlanStep::CreateInterface(CreateInterface {
                device: device_ref(&device),
                name: "lo-test".into(),
                kind: "virtual".into(),
            }),
            PlanStep::CreateAddress(CreateAddress {
                address: "192.0.2.1/32".into(),
                device: device_ref(&device),
                interface: "lo-test".into(),
                prefix: "192.0.2.0/24".into(),
                on: Target::Step(0),
            }),
            PlanStep::SetPrimaryIp(SetPrimaryIp {
                device: device_ref(&device),
                version: IpVersion::V4,
                address: "192.0.2.1/32".into(),
                before: None,
                address_id: Target::Step(1),
                before_id: device.primary_ip_v4().map(|ip| ip.id().0),
            }),
        ];
        let after = plan.preview_device().expect("preview");
        let lo = after
            .interfaces()
            .into_iter()
            .find(|i| i.name() == "lo-test")
            .expect("created interface");
        let address = lo.ips().first().and_then(|ip| ip.net()).map(|n| n.to_string());
        assert_eq!(address.as_deref(), Some("192.0.2.1/32"));
        let primary = after.primary_ip_v4().and_then(|ip| ip.net()).map(|n| n.to_string());
        assert_eq!(primary.as_deref(), Some("192.0.2.1/32"));
        assert!(!device.interfaces().iter().any(|i| i.name() == "lo-test"));
    }

    /// A link's steps in the preview: the cable joins the two ports, both have their address
    /// in the new prefix, label, MTU and `ospf`.
    #[test]
    fn preview_has_the_new_link() {
        let topology = example_topology();
        let free_port = |not: Option<DeviceId>| {
            topology
                .list_devices()
                .filter(|d| d.has_routeros() && Some(d.id()) != not)
                .flat_map(|d| d.interfaces().into_vec())
                .find(|i| {
                    i.is_ethernet_port()
                        && i.cable().is_none()
                        && i.ips().is_empty()
                        && i.bridge().is_none()
                        && !i.use_ospf()
                })
                .expect("a free port")
        };
        let a = free_port(None);
        let b = free_port(a.device().map(|d| d.id()));
        let (a_device, b_device) = (a.device().unwrap(), b.device().unwrap());
        let mut plan = NetboxPlan::new(&a_device, "test");
        let update = |end: &InterfaceAccess, far: &DeviceAccess| {
            PlanStep::UpdateInterface(UpdateInterface {
                interface: interface_ref(end),
                label: Some(far.name().into()),
                label_before: end.label().map(Box::from),
                mtu: Some(2000),
                mtu_before: end.mtu(),
                ospf: true,
            })
        };
        let address = |end: &InterfaceAccess, address: &str| {
            PlanStep::CreateAddress(CreateAddress {
                address: address.into(),
                device: device_ref(&end.device().unwrap()),
                interface: end.name().into(),
                prefix: "192.0.2.0/31".into(),
                on: Target::Existing(end.id().0),
            })
        };
        plan.steps = vec![
            PlanStep::CreateCable(CreateCable {
                a: interface_ref(&a),
                b: interface_ref(&b),
            }),
            PlanStep::CreatePrefix(CreatePrefix {
                prefix: "192.0.2.0/31".into(),
                pool: "192.0.2.0/24".into(),
                description: "test".into(),
            }),
            address(&a, "192.0.2.0/31"),
            address(&b, "192.0.2.1/31"),
            update(&a, &b_device),
            update(&b, &a_device),
        ];
        let after = plan.preview_topology().expect("preview");
        let a_after = after.get_interface_by_id(a.id()).unwrap();
        let connected = a_after.connected_interfaces();
        assert_eq!(connected.iter().map(|i| i.id()).collect::<Vec<_>>(), [b.id()]);
        for (end, address, far) in [(&a, "192.0.2.0/31", &b_device), (&b, "192.0.2.1/31", &a_device)] {
            let end = after.get_interface_by_id(end.id()).unwrap();
            let net = end.ips().first().and_then(|ip| ip.net()).map(|n| n.to_string());
            assert_eq!(net.as_deref(), Some(address));
            let prefix = end.ips().first().and_then(|ip| ip.prefix()).and_then(|p| p.prefix());
            assert_eq!(prefix.map(|p| p.to_string()).as_deref(), Some("192.0.2.0/31"));
            assert_eq!(end.label(), Some(far.name()));
            assert_eq!(end.mtu(), Some(2000));
            assert!(end.use_ospf());
        }
    }
}
