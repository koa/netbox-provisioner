//! Helper „Neues Gerät“: a RouterOS device for NetBox, cabled to a free port of a router whose
//! page asks (`docs/concepts/config-helper.md#neues-gerät`): one without cable, or one whose
//! cable ends open at a patch panel (`landing`), the new cable then starts there. One plan: the device (device type
//! and role chosen, the router's tenant, whose credentials the initial setup uses, platform
//! RouterOS), the port its cable lands on (NetBox's from the device type where it has it, see
//! `write`), then on the preview „Loopback“ and, for a router, „Verbindung“ to the port; a
//! bridge device only gets the cable, its radio link comes from „Funkbrücke“.
//!
//! The device type is NetBox's: RouterOS reports the board (`RB962UiGS-5HacT2HnT`), NetBox's
//! types are named as MikroTik sells them („hAP ac“), so the operator chooses, the frontend
//! preselects one whose model or part number is the board.
use super::{
    CableEnd, CreateCable, CreateDevice, CreateInterface, NetboxPlan, PlanStep,
    connection::{self, ConnectionInput},
    loopback,
};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{
        CablePort, DeviceId, DeviceRole, InterfaceId, Topology,
        access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess},
        findings::{DeviceRef, InterfaceRef, device_ref, interface_ref},
    },
};
use async_graphql::{InputObject, SimpleObject};
use serde_json::Value;
use std::sync::Arc;

/// The router's free port, the new device's name, device type and role (NetBox ids), the port
/// of it the cable lands on (as RouterOS names it, `ether1`) and, for an access point, its WLAN
/// group.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct NewDeviceInput {
    pub port: u32,
    pub name: String,
    pub device_type: u32,
    pub role: u32,
    pub far_port: String,
    pub wlan_group: Option<u32>,
}

/// What NetBox has for the input's ids, read when planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub device_type: Box<str>,
    pub manufacturer: Box<str>,
    pub role: Box<str>,
    pub role_slug: Box<str>,
    pub platform_id: u32,
    pub platform: Box<str>,
    pub tenant_id: Option<u32>,
}

/// The device types, roles and WLAN groups to choose from.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct NewDeviceOptions {
    pub device_types: Vec<DeviceTypeOption>,
    pub roles: Vec<RoleOption>,
    pub wlan_groups: Vec<WlanGroupOption>,
}

/// A WLAN group with its controller (one without is ignored by the generator).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanGroupOption {
    pub id: u32,
    pub name: Box<str>,
    pub controller: Box<str>,
}

/// A MikroTik device type of NetBox: its model, as MikroTik sells it, and part number.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceTypeOption {
    pub id: u32,
    pub model: Box<str>,
    pub part_number: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RoleOption {
    pub id: u32,
    pub name: Box<str>,
    pub slug: Box<str>,
}

/// The name of the platform every device of the provisioner has (`has_routeros`).
const PLATFORM: &str = "routeros";

pub async fn options(topology: &Arc<Topology>) -> Result<NewDeviceOptions, Error> {
    let rest = NetboxRest::reader()?;
    let types = rest
        .get("dcim/device-types/?manufacturer=mikrotik&limit=1000")
        .await?;
    let roles = rest.get("dcim/device-roles/?limit=1000").await?;
    let results = |value: &Value| value["results"].as_array().cloned().unwrap_or_default();
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(Box::from);
    let mut device_types = results(&types)
        .iter()
        .filter_map(|t| {
            Some(DeviceTypeOption {
                id: t["id"].as_u64()? as u32,
                model: text(&t["model"])?,
                part_number: text(&t["part_number"]),
            })
        })
        .collect::<Vec<_>>();
    device_types.sort_by(|a, b| a.model.cmp(&b.model));
    let mut roles = results(&roles)
        .iter()
        .filter_map(|r| {
            Some(RoleOption {
                id: r["id"].as_u64()? as u32,
                name: text(&r["name"])?,
                slug: text(&r["slug"])?,
            })
        })
        .collect::<Vec<_>>();
    roles.sort_by(|a, b| a.name.cmp(&b.name));
    let mut wlan_groups = topology
        .list_wlan_groups()
        .filter_map(|group| {
            Some(WlanGroupOption {
                id: group.id().0,
                name: group.name()?.into(),
                controller: group.controller()?.name().into(),
            })
        })
        .collect::<Vec<_>>();
    wlan_groups.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(NewDeviceOptions {
        device_types,
        roles,
        wlan_groups,
    })
}

pub async fn plan(topology: &Arc<Topology>, input: &NewDeviceInput) -> Result<NetboxPlan, Error> {
    let (router, port) = router_port(topology, input)?;
    let facts = read_facts(&router, input).await?;
    let mut plan = base(topology, input, &facts)?;
    let preview = plan.preview_topology()?;
    let new_device = preview
        .get_device_by_id(&DeviceId(u32::MAX))
        .ok_or(Error::NoPreview)?;
    let mut routes = None;
    loopback::add_to(&mut plan, &new_device, &mut routes).await?;
    if !is_bridge(&facts) {
        // the link to the router, on the preview with the device and its loopback
        let preview = plan.preview_topology()?;
        let connection = connection::plan(
            &preview,
            &ConnectionInput {
                interface: port.id().0,
                peer: new_port_id(),
                mtu: None,
            },
        )
        .await?;
        let shift = plan.steps.len();
        plan.steps.extend(connection.steps.into_iter().map(|mut step| {
            step.shift(shift);
            step
        }));
        plan.undocumented.extend(connection.undocumented);
        plan.cut_off.extend(connection.cut_off);
        plan.checked_with = plan.checked_with.or(connection.checked_with);
    }
    Ok(plan)
}

/// Where a new device's cable starts at a router's port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    /// the port has no cable
    Port,
    /// its one cable path ends at this port of a patch panel, with no cable on
    Panel(CableEnd, InterfaceRef),
}

/// Where a device NetBox doesn't have yet would be cabled to `port`, `None` when something is
/// there: a cable to an interface or a circuit, more than one path.
pub fn landing(port: &InterfaceAccess) -> Option<Landing> {
    if port.cable().is_none() {
        return Some(Landing::Port);
    }
    let paths = port.cable_port().collect_cables();
    let [path] = &paths[..] else { return None };
    let far = path.far_port();
    if far.cable().is_some() {
        return None;
    }
    let (end, id) = match far.port() {
        CablePort::FrontPort(id) => (CableEnd::FrontPort, id.0),
        CablePort::RearPort(id) => (CableEnd::RearPort, id.0),
        _ => return None,
    };
    let device = far.device()?;
    Some(Landing::Panel(
        end,
        InterfaceRef {
            id,
            name: far.name().unwrap_or_default().into(),
            device: device_ref(&device),
        },
    ))
}

/// The router and its free port.
fn router_port(
    topology: &Arc<Topology>,
    input: &NewDeviceInput,
) -> Result<(DeviceAccess, InterfaceAccess), Error> {
    let port = topology
        .get_interface_by_id(InterfaceId(input.port))
        .ok_or(Error::UnknownInterface(input.port))?;
    let router = port.device().ok_or(Error::UnknownInterface(input.port))?;
    if !router.has_routeros() {
        return Err(Error::NoRouterOs(router.name().into()));
    }
    if landing(&port).is_none() {
        return Err(Error::PortCabled {
            port: super::port_name(&port),
            far: port
                .connected_interfaces()
                .first()
                .map(super::port_name),
        });
    }
    Ok((router, port))
}

/// NetBox's device type, role and platform for the input, and the router's tenant: its own,
/// else its location's (without either it is the site's, which the new device gets too).
async fn read_facts(router: &DeviceAccess, input: &NewDeviceInput) -> Result<Facts, Error> {
    let rest = NetboxRest::reader()?;
    let device_type = rest
        .get(&format!("dcim/device-types/{}/", input.device_type))
        .await?;
    let role = rest.get(&format!("dcim/device-roles/{}/", input.role)).await?;
    let platform = rest.get(&format!("dcim/platforms/?name={PLATFORM}")).await?;
    let neighbor = rest.get(&format!("dcim/devices/{}/", router.id().0)).await?;
    let mut tenant_id = neighbor["tenant"]["id"].as_u64();
    if tenant_id.is_none()
        && let Some(location) = neighbor["location"]["id"].as_u64()
    {
        let location = rest.get(&format!("dcim/locations/{location}/")).await?;
        tenant_id = location["tenant"]["id"].as_u64();
    }
    let text = |value: &Value| value.as_str().map(Box::from).unwrap_or_default();
    Ok(Facts {
        device_type: text(&device_type["model"]),
        manufacturer: text(&device_type["manufacturer"]["slug"]),
        role: text(&role["name"]),
        role_slug: text(&role["slug"]),
        platform_id: platform["results"][0]["id"]
            .as_u64()
            .ok_or_else(|| Error::NoPlatform(PLATFORM.into()))? as u32,
        platform: text(&platform["results"][0]["name"]),
        tenant_id: tenant_id.map(|id| id as u32),
    })
}

/// The device and the port its cable lands on; the cable for a bridge device or from a patch
/// panel (else a router gets it from „Verbindung“, which then sees the path).
pub(crate) fn base(
    topology: &Arc<Topology>,
    input: &NewDeviceInput,
    facts: &Facts,
) -> Result<NetboxPlan, Error> {
    let (router, port) = router_port(topology, input)?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::NoDeviceName);
    }
    if topology.list_devices().any(|d| d.name().eq_ignore_ascii_case(name)) {
        return Err(Error::DeviceNameTaken(name.into()));
    }
    let far_port = input.far_port.trim();
    let site = router.site().ok_or(Error::NoSite(router.name().into()))?;
    let wlan_group = match input.wlan_group {
        Some(id) => Some(
            topology
                .list_wlan_groups()
                .find(|group| group.id().0 == id && group.controller().is_some())
                .ok_or(Error::UnknownWlanGroup(id))?,
        ),
        None => None,
    };
    let mut plan = NetboxPlan::new(
        &router,
        format!("Neues Gerät {name} an {} {}", router.name(), port.name()),
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
        platform_id: facts.platform_id,
        platform: facts.platform.clone(),
        tenant_id: facts.tenant_id,
        wlan_group: wlan_group.as_ref().and_then(|g| g.name()).map(Box::from),
        wlan_group_id: wlan_group.as_ref().map(|g| g.id().0),
        template: router.id().0,
    }));
    let device = DeviceRef {
        id: u32::MAX,
        name: name.into(),
        site: Some(site.id().0),
    };
    plan.steps.push(PlanStep::CreateInterface(CreateInterface {
        device: device.clone(),
        name: far_port.into(),
        kind: port_type(far_port).into(),
        vlan: None,
        tags: Vec::new(),
    }));
    let new_port = InterfaceRef {
        id: new_port_id(),
        name: far_port.into(),
        device,
    };
    match landing(&port) {
        Some(Landing::Panel(a_end, panel)) => plan.steps.push(PlanStep::CreateCable(CreateCable {
            a: panel,
            b: new_port,
            a_end,
        })),
        _ if is_bridge(facts) => plan.steps.push(PlanStep::CreateCable(CreateCable {
            a: interface_ref(&port),
            b: new_port,
            a_end: CableEnd::Interface,
        })),
        _ => {}
    }
    Ok(plan)
}

/// A radio bridge device: no link of its own to the router, „Funkbrücke“ makes one.
fn is_bridge(facts: &Facts) -> bool {
    DeviceRole::from_slug(&facts.role_slug) == Some(DeviceRole::WlanBridge)
}

/// The id the preview gives the new device's port (step 1).
fn new_port_id() -> u32 {
    u32::MAX - 1
}

/// NetBox's type for a port named as RouterOS does.
fn port_type(name: &str) -> &'static str {
    if name.starts_with("sfp28") {
        "25gbase-x-sfp28"
    } else if name.starts_with("sfp-sfpplus") || name.starts_with("sfpplus") {
        "10gbase-x-sfpp"
    } else if name.starts_with("sfp") {
        "1000base-x-sfp"
    } else if name.starts_with("qsfp") {
        "40gbase-x-qsfpp"
    } else {
        "1000base-t"
    }
}

#[cfg(test)]
mod test;
