//! Helper „Verbindung abräumen“: takes a link NetBox has wrong out of NetBox, so „Verbindung“
//! can make the right one. For a port: its cable, its addresses from the transfer pool and those
//! of the other end in the same network, and each such network's prefix once nothing else is in
//! it. Other addresses (a customer network) and the pool's own prefixes stay. With the cable go
//! the ends' label where it names the other end (the port's name on the device comes from it)
//! and their `ospf`. Applying then takes the addresses off the devices: a device reached only over this link is cut off until the new
//! link is applied at both ends (`NetboxPlan.cut_off`).
use super::{
    DeleteAddress, DeleteCable, DeletePrefix, NetboxPlan, PlanStep, UpdateInterface, device_ref,
    flags::InterfaceTag,
};
use crate::{
    Error,
    topology::{
        DeviceId, InterfaceId, PrefixPool, Topology,
        access::{AccessTopology, interface::InterfaceAccess, ip_addresses::IpAddressAccess},
        findings::interface_ref,
    },
};
use async_graphql::InputObject;
use ipnet::IpNet;
use std::sync::Arc;

/// The port whose link goes, for the device whose page asks (its job writes it): the port may be
/// the other end's, a link NetBox has to one of the device's ports.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct DisconnectInput {
    pub device: u32,
    pub interface: u32,
}

pub fn plan(topology: &Arc<Topology>, input: &DisconnectInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let port = topology
        .get_interface_by_id(InterfaceId(input.interface))
        .ok_or(Error::UnknownInterface(input.interface))?;
    let port_device = port.device().ok_or(Error::UnknownInterface(input.interface))?;
    let far = port.connected_interfaces().first().cloned();
    let mut plan = NetboxPlan::new(
        &device,
        format!("Verbindung abräumen {} {}", port_device.name(), port.name()),
    );

    if let Some(cable) = port.cable() {
        plan.steps.push(PlanStep::DeleteCable(DeleteCable {
            id: cable.id().0,
            port: interface_ref(&port),
            far: far.as_ref().map(interface_ref),
        }));
    }
    let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
    let in_pool = |ip: &IpAddressAccess| {
        ip.net()
            .is_some_and(|net| pools.iter().any(|(_, pool)| pool.contains(&net.addr())))
    };
    let own: Vec<IpAddressAccess> = port.ips().into_iter().filter(in_pool).collect();
    let networks: Vec<IpNet> = own.iter().filter_map(|ip| ip.net()).map(|net| net.trunc()).collect();
    let far_ips: Vec<IpAddressAccess> = far
        .iter()
        .flat_map(|far| far.ips().into_vec())
        .filter(in_pool)
        .filter(|ip| ip.net().is_some_and(|net| networks.contains(&net.trunc())))
        .collect();
    let freed: Vec<(&IpAddressAccess, &InterfaceAccess)> = own
        .iter()
        .map(|ip| (ip, &port))
        .chain(far_ips.iter().zip(far.iter().cycle()))
        .collect();
    for (ip, interface) in &freed {
        let owner = interface.device().ok_or(Error::UnknownInterface(interface.id().0))?;
        let primary = [owner.primary_ip_v4(), owner.primary_ip_v6()];
        if primary.iter().flatten().any(|primary| primary.id() == ip.id()) {
            return Err(Error::PrimaryAddress {
                address: address_text(ip),
                device: owner.name().into(),
            });
        }
        plan.steps.push(PlanStep::DeleteAddress(DeleteAddress {
            id: ip.id().0,
            address: address_text(ip),
            interface: interface_ref(interface),
        }));
    }
    // a link's own prefix, not a pool's: gone once the addresses freed were all it had
    let mut prefixes = Vec::new();
    for (ip, _) in &freed {
        let Some(prefix) = ip.prefix() else { continue };
        let (Some(net), Some(data)) = (prefix.prefix(), prefix.data()) else {
            continue;
        };
        let only_freed = data
            .addresses
            .iter()
            .all(|id| freed.iter().any(|(ip, _)| ip.id() == *id));
        if data.pool.is_none()
            && Some(net) == ip.net().map(|net| net.trunc())
            && data.children.is_empty()
            && data.ranges.is_empty()
            && only_freed
            && !prefixes.contains(&prefix.id().0)
        {
            prefixes.push(prefix.id().0);
            plan.steps.push(PlanStep::DeletePrefix(DeletePrefix {
                id: prefix.id().0,
                prefix: net.to_string().into(),
            }));
        }
    }
    if let (Some(_), Some(far)) = (port.cable(), &far) {
        for (end, other) in [(&port, far), (far, &port)] {
            let other_name = other.device().map(|device| device.name().to_string());
            let label = end.label().filter(|label| Some(*label) == other_name.as_deref());
            if label.is_some() || end.use_ospf() {
                plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
                    interface: interface_ref(end),
                    label: label.map(|_| "".into()),
                    label_before: end.label().map(Box::from),
                    mtu: None,
                    mtu_before: end.mtu(),
                    tags_added: Vec::new(),
                    tags_removed: end.use_ospf().then_some(InterfaceTag::Ospf).into_iter().collect(),
                }));
            }
        }
    }
    if plan.steps.is_empty() {
        return Err(Error::NothingToDisconnect(
            format!("{} {}", port_device.name(), port.name()).into(),
        ));
    }
    if !freed.is_empty() {
        let ends = std::iter::once(&port).chain(far.iter());
        for end in ends {
            if let Some(owner) = end.device()
                && !has_other_link(&owner, end)
                && !plan.cut_off.iter().any(|cut| cut.id == owner.id().0)
            {
                plan.cut_off.push(device_ref(&owner));
            }
        }
    }
    Ok(plan)
}

/// Whether `device` has a link to a router besides `end`: a port cabled to one, with an IPv4
/// address in a network with it.
fn has_other_link(device: &crate::topology::access::device::DeviceAccess, end: &InterfaceAccess) -> bool {
    device.interfaces().iter().filter(|i| i.id() != end.id()).any(|interface| {
        interface.connected_interfaces().iter().any(|far| {
            far.device().is_some_and(|d| d.has_routeros())
                && interface.ips().iter().filter_map(|ip| ip.net()).any(|own| {
                    far.ips()
                        .iter()
                        .filter_map(|ip| ip.net())
                        .any(|other| own.trunc() == other.trunc())
                })
        })
    })
}

fn address_text(ip: &IpAddressAccess) -> Box<str> {
    ip.net().map(|net| net.to_string()).unwrap_or_default().into()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::device::ros::netbox_example_test::example_topology;

    /// A link between routers in a network of the transfer pool: the plan deletes the cable,
    /// the addresses at both ends and the link's prefix if it has one, and the preview has the
    /// port without them; from either end it is the same.
    #[test]
    fn a_link_goes_with_its_addresses() {
        let topology = example_topology();
        let pools = topology.pool_prefixes(PrefixPool::Transfer, false);
        let in_pool = |ip: &IpAddressAccess| {
            ip.net()
                .is_some_and(|net| pools.iter().any(|(_, pool)| pool.contains(&net.addr())))
        };
        let (port, far) = topology
            .list_devices()
            .filter(|d| d.has_routeros())
            .flat_map(|d| d.interfaces().into_vec())
            .find_map(|port| {
                let far = port.connected_interfaces().first()?.clone();
                let own = port.ips().iter().find(|ip| in_pool(ip))?.net()?;
                let common = far
                    .ips()
                    .iter()
                    .filter(|ip| in_pool(ip))
                    .any(|ip| ip.net().is_some_and(|net| net.trunc() == own.trunc()));
                (common && port.cable().is_some()).then_some((port, far))
            })
            .expect("a link in the transfer pool");
        let device = port.device().unwrap();
        let input = DisconnectInput {
            device: device.id().0,
            interface: port.id().0,
        };
        let plan = plan(&topology, &input).expect("plan");
        assert!(matches!(plan.steps.first(), Some(PlanStep::DeleteCable(_))), "{:?}", plan.steps);
        let deleted_at = |id: InterfaceId| {
            plan.steps.iter().any(|step| {
                matches!(step, PlanStep::DeleteAddress(delete) if delete.interface.id == id.0)
            })
        };
        assert!(deleted_at(port.id()) && deleted_at(far.id()), "{:?}", plan.steps);

        let after = plan.preview_topology().expect("preview");
        let port_after = after.get_interface_by_id(port.id()).unwrap();
        assert!(port_after.cable().is_none());
        assert!(!port_after.ips().iter().any(in_pool));

        let reverse = DisconnectInput {
            device: device.id().0,
            interface: far.id().0,
        };
        assert_eq!(super::plan(&topology, &reverse).expect("plan").steps.len(), plan.steps.len());
    }

    /// A port with nothing to take out of NetBox is refused.
    #[test]
    fn a_free_port_has_nothing_to_disconnect() {
        let topology = example_topology();
        let port = topology
            .list_devices()
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| i.is_ethernet_port() && i.cable().is_none() && i.ips().is_empty())
            .expect("a free port");
        let input = DisconnectInput {
            device: port.device().unwrap().id().0,
            interface: port.id().0,
        };
        assert!(matches!(plan(&topology, &input), Err(Error::NothingToDisconnect(_))));
    }
}
