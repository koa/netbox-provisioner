//! The steps of a [`super::NetboxPlan`]: what it creates, changes or deletes in NetBox.

use super::flags;
use crate::topology::{
    PoeMode, RadioRole,
    findings::{DeviceRef, InterfaceRef, IpVersion, VlanRef},
};
use async_graphql::{SimpleObject, Union};

/// One object a plan creates or changes.
#[derive(Debug, Clone, PartialEq, Eq, Union)]
pub enum PlanStep {
    CreateDevice(CreateDevice),
    CreateInterface(CreateInterface),
    CreateAddress(CreateAddress),
    SetPrimaryIp(SetPrimaryIp),
    CreateCable(CreateCable),
    CreatePrefix(CreatePrefix),
    UpdateInterface(UpdateInterface),
    DeleteCable(DeleteCable),
    DeleteAddress(DeleteAddress),
    DeletePrefix(DeletePrefix),
    UpdatePort(UpdatePort),
    CreateWirelessLink(CreateWirelessLink),
    UpdateWirelessLink(UpdateWirelessLink),
    DeleteWirelessLink(DeleteWirelessLink),
    UpdatePortVlan(UpdatePortVlan),
    DeleteInterface(DeleteInterface),
}

/// An object of NetBox or one an earlier step of the plan creates.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Target {
    Existing(u32),
    Step(usize),
}

impl Target {
    fn shift(&mut self, by: usize) {
        if let Target::Step(step) = self {
            *step += by;
        }
    }
}

impl PlanStep {
    /// The step after `by` steps put in front of its plan: its references to steps move along.
    pub(super) fn shift(&mut self, by: usize) {
        match self {
            PlanStep::CreateAddress(create) => create.on.shift(by),
            PlanStep::SetPrimaryIp(set) => set.address_id.shift(by),
            PlanStep::UpdatePort(update) => {
                if let Some(bridge) = &mut update.bridge_id {
                    bridge.shift(by);
                }
            }
            _ => {}
        }
    }
    /// The step with the ids of objects the plan creates replaced by `real`: a plan computed on
    /// the preview of another one (`helper::new_device`) refers to them by their ids there,
    /// from `u32::MAX` down, which NetBox only has once their step is written. `None` from
    /// `real` for an id that isn't there yet.
    pub(crate) fn with_real_ids(&self, real: impl Fn(u32) -> Option<u32>) -> Option<PlanStep> {
        let device = |device: &mut DeviceRef| -> Option<()> {
            device.id = real(device.id)?;
            Some(())
        };
        let interface = |interface: &mut InterfaceRef| -> Option<()> {
            interface.id = real(interface.id)?;
            device(&mut interface.device)
        };
        let target = |target: &mut Target| -> Option<()> {
            if let Target::Existing(id) = target {
                *id = real(*id)?;
            }
            Some(())
        };
        let mut step = self.clone();
        match &mut step {
            PlanStep::CreateInterface(create) => device(&mut create.device)?,
            PlanStep::CreateAddress(create) => {
                device(&mut create.device)?;
                target(&mut create.on)?;
            }
            PlanStep::SetPrimaryIp(set) => {
                device(&mut set.device)?;
                target(&mut set.address_id)?;
            }
            PlanStep::CreateCable(create) => {
                interface(&mut create.a)?;
                interface(&mut create.b)?;
            }
            PlanStep::UpdateInterface(update) => interface(&mut update.interface)?,
            PlanStep::UpdatePort(update) => {
                interface(&mut update.interface)?;
                if let Some(bridge) = &mut update.bridge_id {
                    target(bridge)?;
                }
            }
            PlanStep::CreateWirelessLink(create) => {
                interface(&mut create.a)?;
                interface(&mut create.b)?;
            }
            PlanStep::UpdatePortVlan(update) => interface(&mut update.interface)?,
            _ => {}
        }
        Some(step)
    }
}

/// A new RouterOS device `name` at the site of `template`, the device it is cabled to: its
/// device type, role and tenant (the template's, which its credentials come from), platform
/// RouterOS. The preview takes the template's site and config context.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateDevice {
    pub name: Box<str>,
    pub site: Box<str>,
    pub device_type: Box<str>,
    pub role: Box<str>,
    #[graphql(skip)]
    pub site_id: u32,
    #[graphql(skip)]
    pub device_type_id: u32,
    /// its manufacturer, for the preview
    #[graphql(skip)]
    pub manufacturer: Box<str>,
    #[graphql(skip)]
    pub role_id: u32,
    #[graphql(skip)]
    pub role_slug: Box<str>,
    /// RouterOS for a device of the provisioner, none for a lease client
    #[graphql(skip)]
    pub platform_id: Option<u32>,
    #[graphql(skip)]
    pub platform: Option<Box<str>>,
    #[graphql(skip)]
    pub tenant_id: Option<u32>,
    /// the WLAN group of an access point (custom field `wlan_group`)
    pub wlan_group: Option<Box<str>>,
    #[graphql(skip)]
    pub wlan_group_id: Option<u32>,
    #[graphql(skip)]
    pub template: u32,
}

/// A new interface `name` of NetBox type `kind` (`virtual`) on `device`, with an untagged
/// VLAN (802.1Q mode access), tags and, for a lease client, its primary MAC address.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateInterface {
    pub device: DeviceRef,
    pub name: Box<str>,
    pub kind: Box<str>,
    #[graphql(skip)]
    pub vlan: Option<u32>,
    pub tags: Vec<flags::InterfaceTag>,
    pub mac: Option<Box<str>>,
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

/// A new cable between two interfaces, or from the open end of a patch panel (`a_end`, `a`
/// then names the panel's port) to an interface.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateCable {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    #[graphql(skip)]
    pub a_end: CableEnd,
}

/// What a cable's end is in NetBox.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum CableEnd {
    #[default]
    Interface,
    FrontPort,
    RearPort,
}

impl CableEnd {
    /// NetBox' object type and the path of its objects below `api/`.
    pub(super) fn netbox(self) -> (&'static str, &'static str) {
        match self {
            CableEnd::Interface => ("dcim.interface", "dcim/interfaces"),
            CableEnd::FrontPort => ("dcim.frontport", "dcim/front-ports"),
            CableEnd::RearPort => ("dcim.rearport", "dcim/rear-ports"),
        }
    }
    /// The `__typename` of the preview's JSON.
    pub(super) fn typename(self) -> &'static str {
        match self {
            CableEnd::Interface => "InterfaceType",
            CableEnd::FrontPort => "FrontPortType",
            CableEnd::RearPort => "RearPortType",
        }
    }
}

/// A new prefix, the next free one of the pool prefix `pool` (or the parent prefix) when the
/// plan was computed (as for `CreateAddress`). It carries no tag: a pool's prefixes have one.
/// A customer network is scoped to its site and names its VLAN.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreatePrefix {
    /// `172.17.1.28/31`
    pub prefix: Box<str>,
    pub pool: Box<str>,
    pub description: Box<str>,
    #[graphql(skip)]
    pub site: Option<u32>,
    #[graphql(skip)]
    pub vlan: Option<u32>,
}

/// Changes of an interface: a new label (empty to take it off), a new MTU, a PoE mode (each
/// none to keep it; `poe_removed` takes the mode off) and tags added or taken off. `poe_before`
/// is checked only with `poe` or `poe_removed`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UpdateInterface {
    pub interface: InterfaceRef,
    pub label: Option<Box<str>>,
    pub label_before: Option<Box<str>>,
    pub mtu: Option<u16>,
    pub mtu_before: Option<u16>,
    pub tags_added: Vec<flags::InterfaceTag>,
    pub tags_removed: Vec<flags::InterfaceTag>,
    pub poe: Option<PoeMode>,
    pub poe_before: Option<PoeMode>,
    pub poe_removed: bool,
}

/// Changes of a port: a new name, an RF role, the bridge it becomes a port of (each none to keep
/// it), or the bridge it leaves. The port must still have its name, `rf_role_before` and
/// `bridge_before`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UpdatePort {
    pub interface: InterfaceRef,
    pub name: Option<Box<str>>,
    pub rf_role: Option<RadioRole>,
    pub rf_role_before: Option<RadioRole>,
    /// the name of the bridge, for reading
    pub bridge: Option<Box<str>>,
    /// the name of the bridge it leaves (`bridge_before`), for reading: no bridge then
    pub leaves_bridge: Option<Box<str>>,
    #[graphql(skip)]
    pub bridge_id: Option<Target>,
    #[graphql(skip)]
    pub bridge_before: Option<u32>,
}

/// A new wireless link between two radio ports, WPA personal with `psk`.
#[derive(Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateWirelessLink {
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    pub ssid: Box<str>,
    #[graphql(skip)]
    pub psk: Box<str>,
}

/// Changes of the wireless link `id` between `a` and `b`: a new SSID (instead of
/// `ssid_before`), a PSK where it has none (WPA personal then). Shown is only that it gets one.
#[derive(Clone, PartialEq, Eq, SimpleObject)]
pub struct UpdateWirelessLink {
    pub id: u32,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
    pub ssid: Option<Box<str>>,
    pub ssid_before: Box<str>,
    pub psk_set: bool,
    #[graphql(skip)]
    pub psk: Option<Box<str>>,
}

// The PSK is left out: it is new each time the plan is computed, which must not change the
// plan's fingerprint (and it isn't shown).
impl std::fmt::Debug for CreateWirelessLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateWirelessLink")
            .field("a", &self.a)
            .field("b", &self.b)
            .field("ssid", &self.ssid)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for UpdateWirelessLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateWirelessLink")
            .field("id", &self.id)
            .field("a", &self.a)
            .field("b", &self.b)
            .field("ssid", &self.ssid)
            .field("ssid_before", &self.ssid_before)
            .field("psk_set", &self.psk_set)
            .finish_non_exhaustive()
    }
}

/// The VLAN of a port: `vlan` untagged (802.1Q mode access), or none (no mode); no tagged
/// VLANs either way. The port must still have `vlan_before` and `tagged_before`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UpdatePortVlan {
    pub interface: InterfaceRef,
    pub vlan: Option<VlanRef>,
    pub vlan_before: Option<VlanRef>,
    pub tagged_before: Vec<VlanRef>,
}

/// The interface `interface`, a virtual one without addresses left (they go in earlier
/// steps).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeleteInterface {
    pub interface: InterfaceRef,
}

/// The wireless link `id` between `a` and `b`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeleteWirelessLink {
    pub id: u32,
    pub a: InterfaceRef,
    pub b: InterfaceRef,
}

/// The cable at `port`, to `far` if it reaches an interface (through patch panels too).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeleteCable {
    pub id: u32,
    pub port: InterfaceRef,
    pub far: Option<InterfaceRef>,
}

/// The IP address `address` (with its length) on `interface`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeleteAddress {
    pub id: u32,
    pub address: Box<str>,
    pub interface: InterfaceRef,
}

/// The prefix `prefix`, empty once the plan's addresses are gone.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeletePrefix {
    pub id: u32,
    pub prefix: Box<str>,
}
