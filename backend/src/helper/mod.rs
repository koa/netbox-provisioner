//! The helpers of `docs/concepts/config-helper.md`: for a standard case, a plan of the NetBox
//! objects to create or change, shown with its effect (`NetboxPlan.preview`) before it is
//! written. They write to NetBox only, never to a device; the device gets it by applying its
//! config as always.
//!
//! A plan is computed again when it is written (`write`): it must be the one shown
//! (`fingerprint`), and each step checks that NetBox is still as the plan found it, so a
//! change made in between stops it instead of being overwritten. Every write is an event of
//! the job (`NetboxWritten`), which is what `undo` takes back.
pub mod loopback;
pub mod routes;

use crate::{
    Error,
    device::AccessibleDevice,
    job::{Event, Log},
    netbox::rest::NetboxRest,
    topology::{
        Topology,
        access::{AdhocCredentials, device::DeviceAccess},
        findings::{DeviceRef, IpVersion},
    },
};
use async_graphql::{ComplexObject, Enum, SimpleObject, Union};
use serde_json::{Value, json};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

/// Which helper computes a plan, for computing it again when it is written.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum Helper {
    /// `loopback`: `lo` with an address of each loopback pool, as primary IPs
    Loopback,
}

impl Helper {
    pub async fn plan(self, device: &DeviceAccess) -> Result<NetboxPlan, Error> {
        match self {
            Helper::Loopback => loopback::plan(device).await,
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
        let device_id = self.device.id().0.to_string();
        for (index, step) in self.steps.iter().enumerate() {
            let devices = json["device_list"].as_array_mut();
            let Some(device) = devices
                .into_iter()
                .flatten()
                .find(|device| device["id"] == device_id.as_str())
            else {
                continue;
            };
            match step {
                PlanStep::CreateInterface(create) => {
                    if let Some(interfaces) = device["interfaces"].as_array_mut() {
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
                    let interface_id = match create.on {
                        Target::Existing(id) => id.to_string(),
                        Target::Step(step) => created_id(step),
                    };
                    if let Some(interface) = device["interfaces"]
                        .as_array_mut()
                        .into_iter()
                        .flatten()
                        .find(|interface| interface["id"] == interface_id.as_str())
                        && let Some(addresses) = interface["ip_addresses"].as_array_mut()
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
                    let address_id = match set.address_id {
                        Target::Existing(id) => id.to_string(),
                        Target::Step(step) => created_id(step),
                    };
                    let field = match set.version {
                        IpVersion::V4 => "primary_ip4",
                        IpVersion::V6 => "primary_ip6",
                    };
                    device[field] = json!({ "id": address_id });
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
    pub interface: Box<str>,
    /// the pool prefix it is taken from
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
    /// what changed, for reading; none for a created object
    pub change: Option<FieldChange>,
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
    helper: Helper,
    device: &DeviceAccess,
    fingerprint: &str,
    user: &str,
    log: &Log,
) -> Result<(), Error> {
    let plan = helper.plan(device).await?;
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
                    change: Some(FieldChange {
                        field: field.into(),
                        value: shown(&object),
                        previous: shown(&current),
                    }),
                }));
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
        change: None,
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
        topology::access::AccessTopology,
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
}
