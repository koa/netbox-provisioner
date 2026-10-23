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
pub mod customer_net;
pub mod disconnect;
pub mod flags;
pub mod loopback;
mod plan;
pub mod poe;
pub mod radio_bridge;
pub mod routes;
mod write;

pub use plan::{
    CreateAddress, CreateCable, CreateInterface, CreatePrefix, CreateWirelessLink, DeleteAddress,
    DeleteCable, DeletePrefix, PlanStep, SetPrimaryIp, Target, UpdateInterface, UpdatePort,
    UpdateWirelessLink,
};
pub use write::{FieldChange, NetboxUndone, NetboxWritten, undo, write};

use crate::{
    Error,
    device::AccessibleDevice,
    topology::{
        DeviceId, Topology,
        access::{AdhocCredentials, device::DeviceAccess, interface::InterfaceAccess},
        findings::{DeviceRef, InterfaceRef, IpVersion},
    },
};
use async_graphql::{ComplexObject, OneofObject, SimpleObject};
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
    /// the link of a port out of NetBox: cable, transfer addresses, their prefix
    Disconnect(disconnect::DisconnectInput),
    /// the tags `ospf`, `dhcp`, `dhcp-client`, `nat` of a device's interfaces
    Flags(flags::FlagsInput),
    /// a network of the device's site on a port or VLAN of it
    CustomerNet(customer_net::CustomerNetInput),
    /// a port feeding another one over PoE
    Poe(poe::PoeInput),
    /// two radio bridge devices between two routers, for the AP's device
    RadioBridge(radio_bridge::RadioBridgeInput),
}

impl Helper {
    pub async fn plan(&self, topology: &Arc<Topology>) -> Result<NetboxPlan, Error> {
        match self {
            Helper::Loopback(_) => loopback::plan(&self.device(topology)?).await,
            Helper::Connection(input) => connection::plan(topology, input).await,
            Helper::Disconnect(input) => disconnect::plan(topology, input),
            Helper::Flags(input) => flags::plan(topology, input),
            Helper::CustomerNet(input) => customer_net::plan(topology, input).await,
            Helper::Poe(input) => poe::plan(topology, input),
            Helper::RadioBridge(input) => radio_bridge::plan(topology, input).await,
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
            Helper::Disconnect(input) => topology
                .get_device_by_id(&DeviceId(input.device))
                .ok_or(Error::UnknownDevice(input.device)),
            Helper::Flags(input) => topology
                .get_device_by_id(&DeviceId(input.device))
                .ok_or(Error::UnknownDevice(input.device)),
            Helper::CustomerNet(input) => topology
                .get_device_by_id(&DeviceId(input.device))
                .ok_or(Error::UnknownDevice(input.device)),
            Helper::Poe(input) => topology
                .get_device_by_id(&DeviceId(input.device))
                .ok_or(Error::UnknownDevice(input.device)),
            Helper::RadioBridge(input) => topology
                .get_device_by_id(&DeviceId(input.ap))
                .ok_or(Error::UnknownDevice(input.ap)),
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
    /// devices whose only link to a router the plan takes the addresses from: applying cuts
    /// them off until a new link is applied at both ends
    pub cut_off: Vec<DeviceRef>,
    /// the two ends of a radio link whose radio settings change: they hear each other only once
    /// both have them, so only applying them together works (one after the other, the first
    /// one's fallback brings its old settings back)
    pub apply_together: Vec<DeviceRef>,
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
            cut_off: Vec::new(),
            apply_together: Vec::new(),
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
                            "tags": create.tags.iter().map(|tag| json!({ "slug": tag.slug() })).collect::<Vec<_>>(),
                            "untagged_vlan": create.vlan.map(|vlan| json!({ "id": vlan.to_string() })),
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
                            "scope": create.site.map(|site| json!({ "__typename": "SiteType", "id": site.to_string() })),
                        }));
                    }
                }
                PlanStep::DeleteCable(delete) => {
                    let id = delete.id.to_string();
                    if let Some(cables) = json["cable_list"].as_array_mut() {
                        cables.retain(|cable| cable["id"] != id.as_str());
                    }
                }
                PlanStep::DeleteAddress(delete) => {
                    let id = delete.id.to_string();
                    if let Some(addresses) = interface(json, &delete.interface.id.to_string())
                        .and_then(|interface| interface["ip_addresses"].as_array_mut())
                    {
                        addresses.retain(|address| address["id"] != id.as_str());
                    }
                    if let Some(addresses) = json["ip_address_list"].as_array_mut() {
                        addresses.retain(|address| address["id"] != id.as_str());
                    }
                }
                PlanStep::DeletePrefix(delete) => {
                    let id = delete.id.to_string();
                    if let Some(prefixes) = json["prefix_list"].as_array_mut() {
                        prefixes.retain(|prefix| prefix["id"] != id.as_str());
                    }
                }
                PlanStep::UpdatePort(update) => {
                    if let Some(interface) = interface(json, &update.interface.id.to_string()) {
                        if let Some(name) = &update.name {
                            interface["name"] = json!(name);
                        }
                        if let Some(role) = update.rf_role {
                            interface["rf_role"] = json!(role.value());
                        }
                        if let Some(bridge) = update.bridge_id {
                            interface["bridge"] = json!({ "id": target_id(bridge) });
                        }
                    }
                }
                PlanStep::CreateWirelessLink(create) => {
                    let link = json!({
                        "id": created_id(index),
                        "interface_a": { "id": create.a.id.to_string() },
                        "interface_b": { "id": create.b.id.to_string() },
                        "ssid": create.ssid,
                        "auth_type": "wpa-personal",
                        "auth_psk": create.psk,
                        "custom_field_data": {},
                    });
                    for end in [&create.a, &create.b] {
                        if let Some(interface) = interface(json, &end.id.to_string()) {
                            interface["wireless_link"] = link.clone();
                        }
                    }
                }
                PlanStep::UpdateWirelessLink(update) => {
                    for end in [&update.a, &update.b] {
                        if let Some(interface) = interface(json, &end.id.to_string()) {
                            let link = &mut interface["wireless_link"];
                            if let Some(ssid) = &update.ssid {
                                link["ssid"] = json!(ssid);
                            }
                            if let Some(psk) = &update.psk {
                                link["auth_type"] = json!("wpa-personal");
                                link["auth_psk"] = json!(psk);
                            }
                        }
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
                        if let Some(poe) = update.poe {
                            interface["poe_mode"] = json!(poe.value());
                        }
                        if let Some(tags) = interface["tags"].as_array_mut() {
                            for tag in &update.tags_removed {
                                tags.retain(|t| t["slug"] != tag.slug());
                            }
                            for tag in &update.tags_added {
                                tags.push(json!({ "slug": tag.slug() }));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A random WPA key of 24 letters and digits.
pub(crate) fn new_psk() -> Box<str> {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..24)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect::<String>()
        .into()
}

/// `device port`, for messages.
pub(crate) fn port_name(interface: &InterfaceAccess) -> Box<str> {
    let device = interface.device();
    format!(
        "{} {}",
        device.as_ref().map(|d| d.name()).unwrap_or_default(),
        interface.name()
    )
    .into()
}

/// The device, as `DeviceRef`, for a plan step.
pub(crate) fn device_ref(device: &DeviceAccess) -> DeviceRef {
    crate::topology::findings::device_ref(device)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        topology::test::example_topology,
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
                vlan: None,
                tags: Vec::new(),
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
                tags_added: vec![flags::InterfaceTag::Ospf],
                tags_removed: Vec::new(),
                poe: None,
                poe_before: None,
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
                site: None,
                vlan: None,
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
