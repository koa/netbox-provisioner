//! Helper „Lease übernehmen“: a static DHCP lease only the router has (the interface view's live
//! check, `LiveRow.device_only_leases`) as its client in NetBox, so the next apply keeps it
//! instead of removing it (`docs/config-helper.md#lease-übernehmen`). A lease is a
//! device without RouterOS whose interface has a MAC and an address of the router's DHCP
//! network (`device::ros::dhcp::static_leases`): the plan creates the device (any device type
//! and role, the router's site, no platform), its interface `eth0` with the MAC as primary one,
//! and the address with the length of the network.
use super::{CreateAddress, CreateDevice, CreateInterface, NetboxPlan, PlanStep, Target};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{DeviceId, Topology, access::AccessTopology, findings::DeviceRef},
};
use async_graphql::{InputObject, SimpleObject};
use ipnet::IpNet;
use mikrotik_model::MacAddress;
use serde_json::Value;
use std::{net::IpAddr, sync::Arc};

/// The lease of the router `device`: its address and MAC, the client's name, device type and
/// role (NetBox ids).
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct LeaseTakeoverInput {
    pub device: u32,
    pub address: String,
    pub mac: String,
    pub name: String,
    pub device_type: u32,
    pub role: u32,
}

/// What NetBox has for the input's ids, read when planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub device_type: Box<str>,
    pub manufacturer: Box<str>,
    pub role: Box<str>,
    pub role_slug: Box<str>,
}

/// The device types (of every manufacturer) and roles to choose from.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LeaseTakeoverOptions {
    pub device_types: Vec<ClientTypeOption>,
    pub roles: Vec<super::new_device::RoleOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct ClientTypeOption {
    pub id: u32,
    pub model: Box<str>,
    pub manufacturer: Box<str>,
}

/// The name of the interface the client gets in NetBox.
const INTERFACE: &str = "eth0";

pub async fn options() -> Result<LeaseTakeoverOptions, Error> {
    let rest = NetboxRest::reader()?;
    let types = rest.get("dcim/device-types/?limit=1000").await?;
    let roles = rest.get("dcim/device-roles/?limit=1000").await?;
    let results = |value: &Value| value["results"].as_array().cloned().unwrap_or_default();
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(Box::from);
    let mut device_types = results(&types)
        .iter()
        .filter_map(|t| {
            Some(ClientTypeOption {
                id: t["id"].as_u64()? as u32,
                model: text(&t["model"])?,
                manufacturer: text(&t["manufacturer"]["name"]).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    device_types.sort_by(|a, b| (&a.manufacturer, &a.model).cmp(&(&b.manufacturer, &b.model)));
    let mut roles = results(&roles)
        .iter()
        .filter_map(|r| {
            Some(super::new_device::RoleOption {
                id: r["id"].as_u64()? as u32,
                name: text(&r["name"])?,
                slug: text(&r["slug"])?,
            })
        })
        .collect::<Vec<_>>();
    roles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(LeaseTakeoverOptions { device_types, roles })
}

pub async fn plan(topology: &Arc<Topology>, input: &LeaseTakeoverInput) -> Result<NetboxPlan, Error> {
    let rest = NetboxRest::reader()?;
    let device_type = rest
        .get(&format!("dcim/device-types/{}/", input.device_type))
        .await?;
    let role = rest.get(&format!("dcim/device-roles/{}/", input.role)).await?;
    let text = |value: &Value| value.as_str().map(Box::from).unwrap_or_default();
    let facts = Facts {
        device_type: text(&device_type["model"]),
        manufacturer: text(&device_type["manufacturer"]["slug"]),
        role: text(&role["name"]),
        role_slug: text(&role["slug"]),
    };
    base(topology, input, &facts)
}

/// The plan on what NetBox has for the input's ids.
pub(crate) fn base(
    topology: &Arc<Topology>,
    input: &LeaseTakeoverInput,
    facts: &Facts,
) -> Result<NetboxPlan, Error> {
    let router = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    if !router.has_routeros() {
        return Err(Error::NoRouterOs(router.name().into()));
    }
    let address = input
        .address
        .trim()
        .parse::<IpAddr>()
        .map_err(|_| Error::InvalidAddress(input.address.as_str().into()))?;
    if topology.ip_address_with_host(address).is_some() {
        return Err(Error::AddressInNetbox(address.to_string().into()));
    }
    let mac = input
        .mac
        .trim()
        .parse::<MacAddress>()
        .map_err(|_| Error::InvalidMac(input.mac.as_str().into()))?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::NoDeviceName);
    }
    if topology.list_devices().any(|d| d.name().eq_ignore_ascii_case(name)) {
        return Err(Error::DeviceNameTaken(name.into()));
    }
    // the network of the router's DHCP server that holds it
    let network = router
        .interfaces()
        .iter()
        .filter(|i| i.is_enable_dhcp_server())
        .flat_map(|i| i.ips())
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V4(_)) && net.contains(&address))
        .ok_or_else(|| Error::NoDhcpNetwork(format!("{} {address}", router.name()).into()))?
        .trunc();
    let site = router.site().ok_or(Error::NoSite(router.name().into()))?;
    let mut plan = NetboxPlan::new(
        &router,
        format!("Lease übernehmen {name} {address} an {}", router.name()),
    );
    plan.steps.push(PlanStep::CreateDevice(CreateDevice {
        name: name.into(),
        site: site.name().into(),
        device_type: facts.device_type.clone(),
        role: facts.role.clone(),
        site_id: site.id().0,
        device_type_id: input.device_type,
        manufacturer: facts.manufacturer.clone(),
        role_id: input.role,
        role_slug: facts.role_slug.clone(),
        platform_id: None,
        platform: None,
        tenant_id: None,
        wlan_group: None,
        wlan_group_id: None,
        template: router.id().0,
    }));
    let client = DeviceRef {
        id: u32::MAX,
        name: name.into(),
        site: Some(site.id().0),
    };
    plan.steps.push(PlanStep::CreateInterface(CreateInterface {
        device: client.clone(),
        name: INTERFACE.into(),
        kind: "other".into(),
        vlan: None,
        tags: Vec::new(),
        mac: Some(mac.to_string().to_ascii_uppercase().into()),
    }));
    plan.steps.push(PlanStep::CreateAddress(CreateAddress {
        address: format!("{address}/{}", network.prefix_len()).into(),
        device: client,
        interface: INTERFACE.into(),
        prefix: network.to_string().into(),
        on: Target::Step(1),
    }));
    Ok(plan)
}

#[cfg(test)]
mod test;
