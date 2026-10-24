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
}

/// A new interface `name` of NetBox type `kind` (`virtual`) on `device`, with an untagged
/// VLAN (802.1Q mode access) and tags.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CreateInterface {
    pub device: DeviceRef,
    pub name: Box<str>,
    pub kind: Box<str>,
    #[graphql(skip)]
    pub vlan: Option<u32>,
    pub tags: Vec<flags::InterfaceTag>,
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
/// none to keep it) and tags added or taken off. `poe_before` is checked only with `poe`.
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
