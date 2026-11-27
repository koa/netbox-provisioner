//! Scope: The helper „PoE“; not reading PoE from a device (`device::poe`), which the network
//! map and the live checks use too.
//!
//! Helper „PoE“: who feeds whom. One port powers the device at the other end of its cable
//! (NetBox `poe_mode=pse`), that device's port is fed (`pd`). The two ports are chosen by hand
//! or taken from what a device reports (`observed`: `/interface/ethernet/poe/monitor`
//! `poe-out-status=powered-on`, the far end from NetBox's cable or else from `/ip/neighbor`).
//! Both ports get their mode, and the cable between them if NetBox lacks it. Without the port
//! fed, the feeding one alone gets `pse`: PoE is on before the device at the other end is in
//! NetBox, which „Neues Gerät“ then gives `pd` (or „PoE“ once it is). „Kein PoE“
//! (`plan_remove`) takes a port's mode off again, and the counterpart at its cable's far end.
//!
//! Whether a board has PoE out at that port NetBox doesn't know (it has no board model); a
//! port without it gets no `poe-out` from the generator anyway (`setup_poe`). What a device
//! reports is read in `device::poe`.
use super::{CreateCable, NetboxPlan, PlanStep, UpdateInterface, plan_interface, port_name};
use crate::{
    Error,
    topology::{
        DeviceId, InterfaceId, PoeMode, Topology,
        access::{device::DeviceAccess, interface::InterfaceAccess},
        findings::{InterfaceRef, interface_ref},
        natural::{naturalize, naturalize_interface},
    },
};
use async_graphql::{InputObject, SimpleObject};
use i18n::changelog::Plan;
use std::sync::Arc;

/// The port that feeds and the port fed, for the device of `device` (one of their devices).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct PoeInput {
    pub device: u32,
    /// becomes `pse`
    pub pse: u32,
    /// becomes `pd`; none to switch PoE on at `pse` alone, the other end not known yet
    pub pd: Option<u32>,
}

pub fn plan(topology: &Arc<Topology>, input: &PoeInput) -> Result<NetboxPlan, Error> {
    let end = |id| {
        topology
            .get_interface_by_id(InterfaceId(id))
            .ok_or(Error::UnknownInterface(id))
    };
    let pse = end(input.pse)?;
    let Some(pd) = input.pd else {
        return pse_alone(&pse, input.device);
    };
    let pd = end(pd)?;
    let (Some(pse_device), Some(pd_device)) = (pse.device(), pd.device()) else {
        return Err(Error::UnknownInterface(input.pse));
    };
    if pse_device.id() == pd_device.id() {
        return Err(Error::SameDevice(pse_device.name().into()));
    }
    let device = [&pse_device, &pd_device]
        .into_iter()
        .find(|d| d.id() == DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    for port in [&pse, &pd] {
        if !port.is_ethernet() {
            return Err(Error::NotEthernet(port_name(port)));
        }
    }
    let mut plan = NetboxPlan::new(
        device,
        Plan::Poe {
            pse: super::changelog_port(&pse_device, &pse),
            pd: super::changelog_port(&pd_device, &pd),
        },
    );
    if !pse.connected_interfaces().iter().any(|i| i.id() == pd.id()) {
        for port in [&pse, &pd] {
            if port.cable().is_some() {
                return Err(Error::PortCabled {
                    port: port_name(port),
                    far: port.connected_interfaces().first().map(port_name),
                });
            }
        }
        plan.steps.push(PlanStep::CreateCable(CreateCable {
            a: plan_interface(&pse),
            b: plan_interface(&pd),
            a_end: super::CableEnd::Interface,
        }));
    }
    for (port, mode) in [(&pse, PoeMode::Pse), (&pd, PoeMode::Pd)] {
        set_mode(&mut plan, port, mode);
    }
    Ok(plan)
}

/// `pse` on the port of the device `device`, nothing at the other end.
fn pse_alone(port: &InterfaceAccess, device: u32) -> Result<NetboxPlan, Error> {
    let owner = port
        .device()
        .filter(|owner| owner.id() == DeviceId(device))
        .ok_or(Error::UnknownDevice(device))?;
    if !port.is_ethernet() {
        return Err(Error::NotEthernet(port_name(port)));
    }
    let mut plan = NetboxPlan::new(
        &owner,
        Plan::PoeFeeding {
            port: super::changelog_port(&owner, port),
        },
    );
    set_mode(&mut plan, port, PoeMode::Pse);
    Ok(plan)
}

/// The step giving `port` `mode`, unless it has it.
fn set_mode(plan: &mut NetboxPlan, port: &InterfaceAccess, mode: PoeMode) {
    if port.poe() != Some(mode) {
        plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
            interface: plan_interface(port),
            label: None,
            label_before: port.label().map(Box::from),
            mtu: None,
            mtu_before: port.mtu(),
            tags_added: Box::default(),
            tags_removed: Vec::new(),
            poe: Some(mode),
            poe_before: port.poe(),
            poe_removed: false,
        }));
    }
}

/// A port whose PoE mode goes out of NetBox, for the device of `device` (the port's).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct PoeRemoveInput {
    pub device: u32,
    pub port: u32,
}

/// The port loses its mode, and so does the port at its cable's far end that has the
/// counterpart (the `pd` it feeds, the `pse` feeding it): one end alone says nothing. A far end
/// with the same mode or none stays as it is.
pub fn plan_remove(topology: &Arc<Topology>, input: &PoeRemoveInput) -> Result<NetboxPlan, Error> {
    let port = topology
        .get_interface_by_id(InterfaceId(input.port))
        .ok_or(Error::UnknownInterface(input.port))?;
    let device = port
        .device()
        .filter(|device| device.id() == DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let mut plan = NetboxPlan::new(
        &device,
        Plan::NoPoe {
            port: super::changelog_port(&device, &port),
        },
    );
    let Some(mode) = port.poe() else {
        return Ok(plan);
    };
    let counterpart = match mode {
        PoeMode::Pse => PoeMode::Pd,
        PoeMode::Pd => PoeMode::Pse,
    };
    let far_ends = port
        .connected_interfaces()
        .into_iter()
        .filter(|far| far.poe() == Some(counterpart));
    for end in std::iter::once(port).chain(far_ends) {
        plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
            interface: plan_interface(&end),
            label: None,
            label_before: end.label().map(Box::from),
            mtu: None,
            mtu_before: end.mtu(),
            tags_added: Box::default(),
            tags_removed: Vec::new(),
            poe: None,
            poe_before: end.poe(),
            poe_removed: true,
        }));
    }
    Ok(plan)
}

/// An Ethernet port to choose for „PoE“, with its mode and where its cable goes.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoePort {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
    pub poe: Option<PoeMode>,
    pub cabled: bool,
    pub connected: Box<[InterfaceRef]>,
}

/// Another device of the site, with its Ethernet ports.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoePeer {
    pub id: u32,
    pub name: Box<str>,
    pub ports: Box<[PoePort]>,
}

/// What „PoE“ chooses from: the device's Ethernet ports, and the other devices of its site
/// (and those its cables reach) with theirs.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct PoeOptions {
    pub ports: Box<[PoePort]>,
    pub peers: Box<[PoePeer]>,
}

pub fn options(device: &DeviceAccess) -> PoeOptions {
    let mut peers: Vec<DeviceAccess> = device
        .site()
        .map(|site| site.devices().collect())
        .unwrap_or_default();
    for far in device
        .interfaces()
        .iter()
        .flat_map(InterfaceAccess::connected_interfaces)
        .filter_map(|far| far.device())
    {
        if !peers.iter().any(|d| d.id() == far.id()) {
            peers.push(far);
        }
    }
    peers.retain(|d| d.id() != device.id());
    peers.sort_by_cached_key(|d| naturalize(d.name()));
    PoeOptions {
        ports: poe_ports(device).into(),
        peers: peers
            .iter()
            .map(|peer| PoePeer {
                id: peer.id().0,
                name: peer.name().into(),
                ports: poe_ports(peer).into(),
            })
            .filter(|peer| !peer.ports.is_empty())
            .collect(),
    }
}

fn poe_ports(device: &DeviceAccess) -> Vec<PoePort> {
    let mut ports: Vec<PoePort> = device
        .interfaces()
        .iter()
        .filter(|interface| interface.is_ethernet())
        .map(|interface| PoePort {
            id: interface.id().0,
            name: interface.name().into(),
            label: interface.label().map(Box::from),
            poe: interface.poe(),
            cabled: interface.cable().is_some(),
            connected: interface
                .connected_interfaces()
                .iter()
                .map(interface_ref)
                .collect(),
        })
        .collect();
    ports.sort_by_cached_key(|port| naturalize_interface(&port.name));
    ports
}

#[cfg(test)]
mod test;
