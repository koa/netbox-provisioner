//! Helper „Flags“: the tags `ospf`, `dhcp`, `dhcp-client` and `nat` of a device's interfaces as
//! switches (`docs/netbox-modeling.md`, Tags). The plan sets the ones switched and takes off the
//! ones switched off; the findings of the device after it (`Topology.findings(after:)`) show
//! what doesn't fit then, such as `ospf` at one end of a cable only.
use super::{NetboxPlan, PlanStep, UpdateInterface};
use crate::{
    Error,
    topology::{
        DeviceId, InterfaceId, Topology,
        access::{device::DeviceAccess, interface::InterfaceAccess},
        findings::{InterfaceRef, interface_ref},
        natural::naturalize_interface,
    },
};
use async_graphql::{Enum, InputObject, SimpleObject};
use std::sync::Arc;

/// A tag of an interface that changes its config.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Enum)]
pub enum InterfaceTag {
    /// OSPF (v2 and v3) on the interface
    Ospf,
    /// a DHCP server (IPv4), router advertisements for its IPv6 /64
    Dhcp,
    /// a DHCP client, if the interface has no address
    DhcpClient,
    /// an uplink whose network doesn't route back: masquerade outgoing
    Nat,
}

pub const ALL_TAGS: [InterfaceTag; 4] = [
    InterfaceTag::Ospf,
    InterfaceTag::Dhcp,
    InterfaceTag::DhcpClient,
    InterfaceTag::Nat,
];

impl InterfaceTag {
    pub fn slug(self) -> &'static str {
        match self {
            InterfaceTag::Ospf => "ospf",
            InterfaceTag::Dhcp => "dhcp",
            InterfaceTag::DhcpClient => "dhcp-client",
            InterfaceTag::Nat => "nat",
        }
    }
    pub fn is_set(self, interface: &InterfaceAccess) -> bool {
        match self {
            InterfaceTag::Ospf => interface.use_ospf(),
            InterfaceTag::Dhcp => interface.is_enable_dhcp_server(),
            InterfaceTag::DhcpClient => interface.is_enable_dhcp_client(),
            InterfaceTag::Nat => interface.is_nat(),
        }
    }
}

/// The tags of the device's interfaces as they are to be.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct FlagsInput {
    pub device: u32,
    pub changes: Vec<TagChange>,
}

/// `tag` on `interface` (`set`) or not.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct TagChange {
    pub interface: u32,
    pub tag: InterfaceTag,
    pub set: bool,
}

/// An interface in the table of „Flags“.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct FlagInterface {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
    /// with their length
    pub addresses: Vec<Box<str>>,
    /// the bridge it is a port of: the tags of the bridge count then, not its own
    pub bridge: Option<Box<str>>,
    pub connected: Vec<InterfaceRef>,
    pub tags: Vec<InterfaceTag>,
}

/// The interfaces whose tags count, in NetBox' order: all but the ports of a bridge, which
/// only come with a tag to take off.
pub fn flag_interfaces(device: &DeviceAccess) -> Vec<FlagInterface> {
    let mut interfaces: Vec<FlagInterface> = device
        .interfaces()
        .iter()
        .map(|interface| FlagInterface {
            id: interface.id().0,
            name: interface.name().into(),
            label: interface.label().map(Box::from),
            addresses: interface
                .ips()
                .iter()
                .filter_map(|ip| ip.net())
                .map(|net| net.to_string().into())
                .collect(),
            bridge: interface.bridge().map(|bridge| bridge.name().into()),
            connected: interface.connected_interfaces().iter().map(interface_ref).collect(),
            tags: ALL_TAGS.into_iter().filter(|tag| tag.is_set(interface)).collect(),
        })
        .filter(|interface| interface.bridge.is_none() || !interface.tags.is_empty())
        .collect();
    interfaces.sort_by_cached_key(|interface| naturalize_interface(&interface.name));
    interfaces
}

pub fn plan(topology: &Arc<Topology>, input: &FlagsInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let mut plan = NetboxPlan::new(&device, format!("Flags {}", device.name()));
    let mut interfaces = Vec::<InterfaceAccess>::new();
    for change in &input.changes {
        let interface = topology
            .get_interface_by_id(InterfaceId(change.interface))
            .filter(|interface| interface.device().is_some_and(|d| d.id() == device.id()))
            .ok_or(Error::UnknownInterface(change.interface))?;
        if !interfaces.iter().any(|known| known.id() == interface.id()) {
            interfaces.push(interface);
        }
    }
    interfaces.sort_by_cached_key(|interface| naturalize_interface(interface.name()));
    for interface in interfaces {
        // the last change of a tag counts
        let wanted = |tag: InterfaceTag| {
            input
                .changes
                .iter()
                .rev()
                .find(|change| change.interface == interface.id().0 && change.tag == tag)
                .map(|change| change.set)
        };
        let differs = |set: bool| {
            ALL_TAGS
                .into_iter()
                .filter(|tag| wanted(*tag) == Some(set) && tag.is_set(&interface) != set)
                .collect::<Vec<_>>()
        };
        let (added, removed) = (differs(true), differs(false));
        if added.is_empty() && removed.is_empty() {
            continue;
        }
        plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
            interface: interface_ref(&interface),
            label: None,
            label_before: interface.label().map(Box::from),
            mtu: None,
            mtu_before: interface.mtu(),
            tags_added: added,
            tags_removed: removed,
            poe: None,
            poe_before: None,
        }));
    }
    Ok(plan)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::topology::test::example_topology;

    /// Switching a tag on and another one off is one change of the interface; switching one
    /// to what it is already is none.
    #[test]
    fn switched_tags_are_one_update() {
        let topology = example_topology();
        let interface = topology
            .list_devices()
            .filter(|d| d.has_routeros())
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| i.use_ospf() && !i.is_nat() && i.bridge().is_none())
            .expect("an interface with ospf");
        let device = interface.device().unwrap();
        let change = |tag, set| TagChange {
            interface: interface.id().0,
            tag,
            set,
        };
        let input = FlagsInput {
            device: device.id().0,
            changes: vec![
                change(InterfaceTag::Ospf, false),
                change(InterfaceTag::Nat, true),
                change(InterfaceTag::Ospf, true),
                change(InterfaceTag::Ospf, false),
            ],
        };
        let plan = plan(&topology, &input).expect("plan");
        let [PlanStep::UpdateInterface(update)] = &plan.steps[..] else {
            panic!("{:?}", plan.steps);
        };
        assert_eq!(update.tags_added, [InterfaceTag::Nat]);
        assert_eq!(update.tags_removed, [InterfaceTag::Ospf]);

        let after = plan.preview_topology().expect("preview");
        let interface_after = after.get_interface_by_id(interface.id()).unwrap();
        assert!(interface_after.is_nat() && !interface_after.use_ospf());

        let unchanged = FlagsInput {
            device: device.id().0,
            changes: vec![change(InterfaceTag::Ospf, true)],
        };
        assert!(super::plan(&topology, &unchanged).expect("plan").steps.is_empty());
    }

    /// An interface of another device is refused.
    #[test]
    fn only_the_devices_interfaces() {
        let topology = example_topology();
        let mut routers = topology.list_devices().filter(|d| d.has_routeros());
        let (a, b) = (routers.next().unwrap(), routers.next().unwrap());
        let input = FlagsInput {
            device: a.id().0,
            changes: vec![TagChange {
                interface: b.interfaces()[0].id().0,
                tag: InterfaceTag::Ospf,
                set: true,
            }],
        };
        assert!(matches!(plan(&topology, &input), Err(Error::UnknownInterface(_))));
    }
}
