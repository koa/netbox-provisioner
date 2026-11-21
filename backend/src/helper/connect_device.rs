//! Scope: The helper „Gerät anschliessen“, the resident's `connectDevice`; not who may
//! (`graphql::resident::change`).
//!
//! A RouterOS device's port (its „Anschluss“) to a router's port, directly or through a socket
//! whose rear port leads to that router's port: the link of „Verbindung“ (cable, `/31` of the
//! transfer pool, `ospf`, MTU, labels) and, where it lacks one, its loopback. What was in the
//! way goes first, one after the other on NetBox as the one before leaves it: the device's old
//! link („Verbindung abräumen“ at its port), what is at the router's port or plugged into the
//! socket, and the VLAN of the router's port (a link has addresses, no VLAN). As with „Kabel
//! übernehmen“, the steps of each part are shifted behind those before.
use super::{
    CableEnd, CreateCable, NetboxPlan, PlanStep, UpdatePortVlan,
    connection::{self, ConnectionInput},
    disconnect::{self, DisconnectInput},
    loopback, port_name,
    routes::Routes,
};
use crate::{
    Error,
    topology::{
        FrontPortId, InterfaceId, Topology,
        access::{
            AccessTopology, cable::CablePortAccess, device::DeviceAccess,
            front_port::FrontPortAccess, interface::InterfaceAccess,
        },
        findings::{DeviceRef, InterfaceRef, interface_ref, vlan_ref},
        management::{ManagementTree, shared_interfaces},
        natural::naturalize_interface,
    },
};
use std::sync::Arc;

/// A device's port to a router's port, or to a socket in front of it.
#[derive(Debug, Clone, PartialEq, Eq, async_graphql::InputObject)]
pub struct ConnectDeviceInput {
    /// the router's port the link goes to (for a socket: the one behind its wall)
    pub port: u32,
    /// the socket the device's cable goes into, a front port whose rear port leads to `port`
    pub socket: Option<u32>,
    /// the device's port
    pub device_port: u32,
}

/// The port of `device` its link goes out of (its „Anschluss“): the cabled Ethernet port in a
/// network with its parent in the management tree, else its first Ethernet port (by name)
/// without cable, bridge and address.
pub fn uplink_port(device: &DeviceAccess) -> Option<InterfaceAccess> {
    let tree = ManagementTree::of_device(device);
    let parent = tree
        .parent(device.id())
        .and_then(|parent| device.topology().get_device_by_id(&parent));
    let linked = parent.and_then(|parent| {
        shared_interfaces(device, &parent)
            .into_iter()
            .find(|port| port.is_ethernet_port() && port.cable().is_some())
    });
    linked.or_else(|| {
        let mut ports = device
            .interfaces()
            .into_vec()
            .into_iter()
            .filter(|port| {
                port.is_ethernet_port()
                    && port.cable().is_none()
                    && port.bridge().is_none()
                    && port.ips().is_empty()
            })
            .collect::<Vec<_>>();
        ports.sort_by_cached_key(|port| naturalize_interface(port.name()));
        ports.into_iter().next()
    })
}

pub async fn plan(
    topology: &Arc<Topology>,
    input: &ConnectDeviceInput,
) -> Result<NetboxPlan, Error> {
    let mut plan = prepare(topology, input)?;
    let now = |plan: &NetboxPlan| match plan.steps.is_empty() {
        true => Ok(topology.clone()),
        false => plan.preview_topology(),
    };
    let connect = ConnectionInput {
        interface: input.port,
        peer: input.device_port,
        mtu: None,
    };
    let connection = connection::plan(&now(&plan)?, &connect).await?;
    plan.append(connection);
    // its loopback, where it lacks one: the router ID and management address
    let topology = now(&plan)?;
    let device = topology
        .get_interface_by_id(InterfaceId(input.device_port))
        .and_then(|port| port.device())
        .ok_or(Error::UnknownInterface(input.device_port))?;
    let mut lo = NetboxPlan::new(&device, "");
    let mut routes: Option<Routes> = None;
    loopback::add_to(&mut lo, &device, &mut routes).await?;
    plan.append(lo);
    if plan.steps.is_empty() {
        return Err(Error::NothingToChange);
    }
    Ok(plan)
}

/// What goes before the link: what is in the way cleared, the router's port without VLAN, the
/// cable into the socket; empty where nothing is in the way.
fn prepare(topology: &Arc<Topology>, input: &ConnectDeviceInput) -> Result<NetboxPlan, Error> {
    let interface = |topology: &Arc<Topology>, id| {
        topology
            .get_interface_by_id(InterfaceId(id))
            .ok_or(Error::UnknownInterface(id))
    };
    let port = interface(topology, input.port)?;
    let device_port = interface(topology, input.device_port)?;
    let router = port.device().ok_or(Error::UnknownInterface(input.port))?;
    let device = device_port
        .device()
        .ok_or(Error::UnknownInterface(input.device_port))?;
    if !device.has_routeros() {
        return Err(Error::NoRouterOs(device.name().into()));
    }
    let socket = input
        .socket
        .map(|id| FrontPortAccess::create(topology.clone(), FrontPortId(id)));
    let mut plan = NetboxPlan::new(
        &router,
        format!(
            "Anschluss {} {} an {} {}",
            device.name(),
            device_port.name(),
            router.name(),
            port.name()
        ),
    );
    let now = |plan: &NetboxPlan| match plan.steps.is_empty() {
        true => Ok(topology.clone()),
        false => plan.preview_topology(),
    };
    let reaches = |topology: &Arc<Topology>| -> Result<bool, Error> {
        let device_port = interface(topology, input.device_port)?;
        Ok(device_port
            .connected_interfaces()
            .iter()
            .any(|far| far.id().0 == input.port))
    };
    if !reaches(topology)? {
        // the cables in the way: the device's old link, what is at the router's port (not a
        // socket's rear cable, which leads to it), what is plugged into the socket
        let mut clear = vec![device_port.clone()];
        // its Anschluss so far, when the link moves to another of its ports
        clear.extend(
            uplink_port(&device)
                .filter(|old| old.id() != device_port.id() && old.cable().is_some()),
        );
        match &socket {
            None => clear.push(port.clone()),
            Some(socket) => clear.extend(
                CablePortAccess::FrontPort(socket.clone())
                    .attached_cable_segments()
                    .iter()
                    .filter_map(|segment| match &segment.far {
                        CablePortAccess::Interface(far) => Some(far.clone()),
                        _ => None,
                    }),
            ),
        }
        for end in clear {
            let topology = now(&plan)?;
            let end = interface(&topology, end.id().0)?;
            let Some(owner) = end.device() else { continue };
            let input = DisconnectInput {
                device: owner.id().0,
                interface: end.id().0,
            };
            match disconnect::plan(&topology, &input) {
                Ok(cleared) => plan.append(cleared),
                Err(Error::NothingToDisconnect(_)) => {}
                Err(error) => return Err(error),
            }
        }
    }
    // a link has addresses, no VLAN
    {
        let topology = now(&plan)?;
        let port = interface(&topology, input.port)?;
        let tagged = port.tagged_vlans().collect::<Vec<_>>();
        let before = port.untagged_vlan();
        if before.is_some() || !tagged.is_empty() {
            plan.steps.push(PlanStep::UpdatePortVlan(UpdatePortVlan {
                interface: interface_ref(&port),
                vlan: None,
                vlan_before: before.as_ref().and_then(vlan_ref),
                tagged_before: tagged.iter().filter_map(vlan_ref).collect(),
            }));
        }
    }
    if let Some(socket) = &socket {
        let topology = now(&plan)?;
        if !reaches(&topology)? {
            let device_port = interface(&topology, input.device_port)?;
            let wall = socket
                .device()
                .ok_or(Error::SocketWithoutPort(port_name(&port)))?;
            plan.steps.push(PlanStep::CreateCable(CreateCable {
                a: InterfaceRef {
                    id: AccessTopology::id(socket).0,
                    name: socket.name().unwrap_or_default().into(),
                    device: DeviceRef {
                        id: wall.id().0,
                        name: wall.name().into(),
                        site: wall.site().map(|site| AccessTopology::id(&site).0),
                    },
                },
                b: interface_ref(&device_port),
                a_end: CableEnd::FrontPort,
            }));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod test;
