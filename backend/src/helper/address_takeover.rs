//! Helper „Adresse übernehmen“: an address only the device has (the interface view's live
//! check, `LiveRow.device_only_addresses`) on its NetBox interface, so the next apply keeps it
//! instead of removing it (`docs/interface-view.md`). Its network becomes a prefix
//! when NetBox has none of it and nothing in it, within the smallest one holding it; a host
//! length (`/32`, `/128`) stays in that one.
use super::{CreateAddress, CreatePrefix, NetboxPlan, PlanStep, Target};
use crate::{
    Error,
    topology::{DeviceId, InterfaceId, Topology, findings::device_ref},
};
use async_graphql::InputObject;
use ipnet::IpNet;
use std::sync::Arc;

/// The address with its length (`192.168.1.1/24`) for the device's interface, for the device
/// whose page asks (its job writes it).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct AddressTakeoverInput {
    pub device: u32,
    pub interface: u32,
    pub address: String,
}

pub fn plan(topology: &Arc<Topology>, input: &AddressTakeoverInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let interface = topology
        .get_interface_by_id(InterfaceId(input.interface))
        .filter(|interface| interface.device().is_some_and(|d| d.id() == device.id()))
        .ok_or(Error::UnknownInterface(input.interface))?;
    let address = input
        .address
        .parse::<IpNet>()
        .map_err(|_| Error::InvalidAddress(input.address.as_str().into()))?;
    if topology.ip_address_with_host(address.addr()).is_some() {
        return Err(Error::AddressInNetbox(address.to_string().into()));
    }
    let network = address.trunc();
    let holder = topology.smallest_prefix_holding(network);
    let mut plan = NetboxPlan::new(
        &device,
        format!("Adresse übernehmen {address} an {} {}", device.name(), interface.name()),
    );
    let new_prefix = holder.is_none_or(|holder| holder.prefix != network)
        && network.prefix_len() < network.max_prefix_len()
        && topology.addresses_in(network).is_empty();
    let prefix = if new_prefix {
        plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
            prefix: network.to_string().into(),
            pool: holder.map(|holder| holder.prefix.to_string()).unwrap_or_default().into(),
            description: format!("Übernommen von {} {}", device.name(), interface.name()).into(),
            // a pool's prefixes are no site's, as those of the other helpers
            site: holder
                .is_none_or(|holder| holder.pool.is_none())
                .then(|| device_ref(&device).site)
                .flatten(),
            vlan: None,
            dhcp_lease_time: None,
        }));
        network
    } else {
        holder.map_or(network, |holder| holder.prefix)
    };
    plan.steps.push(PlanStep::CreateAddress(CreateAddress {
        address: address.to_string().into(),
        device: device_ref(&device),
        interface: interface.name().into(),
        prefix: prefix.to_string().into(),
        on: Target::Existing(interface.id().0),
    }));
    Ok(plan)
}

#[cfg(test)]
mod test;
