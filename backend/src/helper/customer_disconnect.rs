//! Helper „Kunden-Anschluss abräumen“: takes a customer port's network out of NetBox, the
//! inverse of „Kunden-Netz“ (`docs/interface-view.md`). From the row's interface (a
//! port, a bridge or a VLAN interface): its addresses, each network's prefix once nothing else
//! is in it, `dhcp`, and the VLANs of the port: of the interface and the ports of its bridge, of
//! a VLAN interface the device's customer ports with its VLAN. A VLAN interface left with
//! nothing goes. The VLAN itself stays, other devices may have it.
use super::{
    DeleteAddress, DeleteInterface, NetboxPlan, PlanStep, UpdateInterface, UpdatePortVlan,
    disconnect::{address_text, delete_prefixes},
    flags::InterfaceTag,
};
use crate::{
    Error,
    topology::{
        DeviceId, InterfaceId, Topology,
        access::{AccessTopology, interface::InterfaceAccess},
        findings::{interface_ref, vlan_ref},
    },
};
use async_graphql::InputObject;
use std::sync::Arc;

/// The row's interface, for the device whose page asks (its job writes it).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct CustomerDisconnectInput {
    pub device: u32,
    pub interface: u32,
}

pub fn plan(topology: &Arc<Topology>, input: &CustomerDisconnectInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let l3 = topology
        .get_interface_by_id(InterfaceId(input.interface))
        .ok_or(Error::UnknownInterface(input.interface))?;
    let owner = l3.device().ok_or(Error::UnknownInterface(input.interface))?;
    let mut plan = NetboxPlan::new(
        &device,
        format!("Kunden-Anschluss abräumen {} {}", owner.name(), l3.name()),
    );

    let primary = [owner.primary_ip_v4(), owner.primary_ip_v6()];
    let addresses = l3.ips().into_vec();
    for ip in &addresses {
        if primary.iter().flatten().any(|primary| primary.id() == ip.id()) {
            return Err(Error::PrimaryAddress {
                address: address_text(ip),
                device: owner.name().into(),
            });
        }
        plan.steps.push(PlanStep::DeleteAddress(DeleteAddress {
            id: ip.id().0,
            address: address_text(ip),
            interface: interface_ref(&l3),
        }));
    }
    delete_prefixes(&mut plan, addresses.iter());

    // a VLAN interface: what it has besides addresses, `dhcp` and its VLAN keeps it
    let vlan_interface = !l3.is_ethernet_port()
        && !l3.is_bridge()
        && l3.radio().is_none()
        && l3.untagged_vlan().is_some();
    let keeps = |interface: &InterfaceAccess| {
        interface.use_ospf()
            || interface.is_nat()
            || interface.is_internet()
            || interface.is_enable_dhcp_client()
            || interface.label().is_some()
            || interface.tagged_vlans().next().is_some()
    };
    let delete = vlan_interface && !keeps(&l3);
    if l3.is_enable_dhcp_server() && !delete {
        plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
            interface: interface_ref(&l3),
            label: None,
            label_before: l3.label().map(Box::from),
            mtu: None,
            mtu_before: l3.mtu(),
            tags_added: Vec::new(),
            tags_removed: vec![InterfaceTag::Dhcp],
            poe: None,
            poe_before: None,
            poe_removed: false,
        }));
    }

    // the ports whose VLANs go: the interface and its bridge's ports, of a VLAN interface the
    // device's customer ports with its VLAN
    let interfaces = owner.interfaces();
    let mut ports = Vec::<InterfaceAccess>::new();
    if !vlan_interface {
        ports.push(l3.clone());
    }
    ports.extend(
        interfaces
            .iter()
            .filter(|i| i.bridge().is_some_and(|b| b.id() == l3.id()))
            .cloned(),
    );
    if vlan_interface && let Some(vlan) = l3.untagged_vlan() {
        ports.extend(
            interfaces
                .iter()
                .filter(|i| i.is_ethernet_port() && i.bridge().is_none())
                .filter(|i| i.untagged_vlan().is_some_and(|v| v.id == vlan.id))
                .filter(|i| {
                    !i.connected_interfaces()
                        .iter()
                        .any(|far| far.device().is_some_and(|d| d.has_routeros()))
                })
                .cloned(),
        );
    }
    for port in &ports {
        let untagged = port.untagged_vlan();
        let tagged = port.tagged_vlans().collect::<Vec<_>>();
        if untagged.is_none() && tagged.is_empty() {
            continue;
        }
        plan.steps.push(PlanStep::UpdatePortVlan(UpdatePortVlan {
            interface: interface_ref(port),
            vlan: None,
            vlan_before: untagged.as_ref().and_then(vlan_ref),
            tagged_before: tagged.iter().filter_map(vlan_ref).collect(),
        }));
    }
    if delete {
        plan.steps.push(PlanStep::DeleteInterface(DeleteInterface {
            interface: interface_ref(&l3),
        }));
    }
    if plan.steps.is_empty() {
        return Err(Error::NoCustomerNet(super::port_name(&l3)));
    }
    Ok(plan)
}

#[cfg(test)]
mod test;
