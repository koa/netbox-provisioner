//! Helper „Kunden-Netz“: a network for a port or a VLAN of a device, out of a prefix of its
//! site chosen by hand (`docs/concepts/config-helper.md`). The next free network of the chosen
//! length in it, scoped to the site, its first address as gateway on the port or on a virtual
//! interface of the VLAN (a new one named like the VLAN, `docs/netbox-modeling.md`), and the
//! tag `dhcp` if wanted. Without an IP range the DHCP pool is the rest of the network.
//!
//! As with the links, a network NetBox has free may be in use: one the network routes in part
//! (`routes::Routes::overlapping`) or with addresses in NetBox is passed over.
use super::{
    CreateAddress, CreateInterface, CreatePrefix, NetboxPlan, PlanStep, Target, UpdateInterface,
    device_ref, flags::InterfaceTag, routes::Routes,
};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{
        DeviceId, InterfaceId, IpPrefixId, Topology,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess,
        },
        findings::interface_ref,
        natural::naturalize_interface,
    },
};
use async_graphql::{InputObject, OneofObject, SimpleObject};
use ipnet::IpNet;
use std::sync::Arc;

/// Where the network goes.
#[derive(Debug, Clone, PartialEq, Eq, OneofObject)]
pub enum CustomerNetTarget {
    /// a port or bridge of the device without IPv4
    Interface(u32),
    /// a VLAN the device has: its virtual interface, or a new one
    Vlan(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct CustomerNetInput {
    pub device: u32,
    pub target: CustomerNetTarget,
    /// the prefix of the site the network is taken from
    pub parent: u32,
    /// the network's length, `/24` without
    pub length: Option<u8>,
    /// the tag `dhcp`: a DHCP server on the network
    pub dhcp: bool,
}

/// What „Kunden-Netz“ can choose from for a device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetOptions {
    pub interfaces: Vec<CustomerNetInterface>,
    pub vlans: Vec<CustomerNetVlan>,
    pub parents: Vec<CustomerNetParent>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetInterface {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetVlan {
    pub id: u32,
    pub name: Box<str>,
    pub vid: u16,
    /// its virtual interface on the device, if it has one already
    pub interface: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetParent {
    pub id: u32,
    pub prefix: Box<str>,
}

const DEFAULT_LENGTH: u8 = 24;

/// The ports and bridges without IPv4 that aren't a link to a router or in a bridge or VLAN,
/// the VLANs of the device without a network on it, and the prefixes of its site.
pub fn options(device: &DeviceAccess) -> CustomerNetOptions {
    let interfaces = device.interfaces();
    let mut free: Vec<CustomerNetInterface> = interfaces
        .iter()
        .filter(|interface| {
            (interface.is_ethernet_port() || interface.is_bridge())
                && ipv4(interface).is_none()
                && interface.bridge().is_none()
                && interface.carried_vlans().is_empty()
                && !interface
                    .connected_interfaces()
                    .iter()
                    .any(|far| far.device().is_some_and(|d| d.has_routeros()))
        })
        .map(|interface| CustomerNetInterface {
            id: interface.id().0,
            name: interface.name().into(),
            label: interface.label().map(Box::from),
        })
        .collect();
    free.sort_by_cached_key(|interface| naturalize_interface(&interface.name));
    let mut vlans: Vec<CustomerNetVlan> = device
        .topology()
        .list_vlans()
        .filter(|vlan| vlan.devices().iter().any(|d| d.id() == device.id()))
        .filter_map(|vlan| {
            let gateway = vlan_interface(device, &vlan);
            if gateway.as_ref().is_some_and(|i| ipv4(i).is_some()) {
                return None;
            }
            Some(CustomerNetVlan {
                id: vlan.id.0,
                name: vlan.name()?.into(),
                vid: vlan.vlan_id()?,
                interface: gateway.map(|i| i.name().into()),
            })
        })
        .collect();
    vlans.sort_by_key(|vlan| (vlan.vid, vlan.name.clone()));
    let parents = device
        .site()
        .map(|site| device.topology().customer_parents(site.id()))
        .unwrap_or_default()
        .into_iter()
        .map(|(id, prefix)| CustomerNetParent {
            id: id.0,
            prefix: prefix.to_string().into(),
        })
        .collect();
    CustomerNetOptions {
        interfaces: free,
        vlans,
        parents,
    }
}

pub async fn plan(topology: &Arc<Topology>, input: &CustomerNetInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let site = device.site().ok_or_else(|| Error::NoSite(device.name().into()))?;
    let parent = topology
        .prefix_data(IpPrefixId(input.parent))
        .filter(|parent| parent.site == Some(site.id()))
        .map(|parent| parent.prefix)
        .ok_or(Error::UnknownPrefix(input.parent))?;
    let length = input.length.unwrap_or(DEFAULT_LENGTH);
    if length <= parent.prefix_len() || length > 30 {
        return Err(Error::NetworkLength {
            length,
            parent: parent.to_string().into(),
        });
    }
    // the interface the gateway goes on, the VLAN it has, how it reads
    let (existing, vlan, what) = match input.target {
        CustomerNetTarget::Interface(id) => {
            let interface = topology
                .get_interface_by_id(InterfaceId(id))
                .filter(|i| i.device().is_some_and(|d| d.id() == device.id()))
                .ok_or(Error::UnknownInterface(id))?;
            let what = match interface.label() {
                Some(label) => format!("{} {} ({label})", device.name(), interface.name()),
                None => format!("{} {}", device.name(), interface.name()),
            };
            (Some(interface), None, what)
        }
        CustomerNetTarget::Vlan(id) => {
            let vlan = topology
                .list_vlans()
                .find(|vlan| vlan.id.0 == id)
                .filter(|vlan| vlan.devices().iter().any(|d| d.id() == device.id()))
                .ok_or(Error::UnknownVlan(id))?;
            let what = format!(
                "{} VLAN {} ({})",
                device.name(),
                vlan.name().unwrap_or_default(),
                vlan.vlan_id().unwrap_or_default()
            );
            (vlan_interface(&device, &vlan), Some(vlan), what)
        }
    };
    if let Some(address) = existing.as_ref().and_then(ipv4) {
        let interface = existing.as_ref().expect("has an address");
        return Err(Error::PortAddressed {
            port: format!("{} {}", device.name(), interface.name()).into(),
            address: address.to_string().into(),
        });
    }
    let mut plan = NetboxPlan::new(&device, format!("Kunden-Netz {what}"));
    let rest = NetboxRest::reader()?;
    let routes = Routes::read(topology).await?;
    plan.checked_with = Some(routes.router.clone());
    let net = next_free(&rest, &routes, &mut plan, input.parent, parent, length).await?;

    let dhcp = input.dhcp.then_some(InterfaceTag::Dhcp);
    let (on, interface_name) = match &existing {
        Some(interface) => (Target::Existing(interface.id().0), Box::from(interface.name())),
        None => {
            let vlan = vlan.as_ref().expect("a VLAN without its interface");
            let name: Box<str> = vlan.name().unwrap_or_default().into();
            plan.steps.push(PlanStep::CreateInterface(CreateInterface {
                device: device_ref(&device),
                name: name.clone(),
                kind: "virtual".into(),
                vlan: Some(vlan.id.0),
                tags: dhcp.into_iter().collect(),
            }));
            (Target::Step(plan.steps.len() - 1), name)
        }
    };
    plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
        prefix: net.to_string().into(),
        pool: parent.to_string().into(),
        description: what.into(),
        site: Some(site.id().0),
        vlan: vlan.as_ref().map(|vlan| vlan.id.0),
    }));
    let gateway = net.hosts().next().ok_or(Error::NoFreeAddress("customer", 4))?;
    plan.steps.push(PlanStep::CreateAddress(CreateAddress {
        address: IpNet::new(gateway, length).expect("a host of the network").to_string().into(),
        device: device_ref(&device),
        interface: interface_name,
        prefix: net.to_string().into(),
        on,
    }));
    if let Some(interface) = &existing
        && input.dhcp
        && !interface.is_enable_dhcp_server()
    {
        plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
            interface: interface_ref(interface),
            label: None,
            label_before: interface.label().map(Box::from),
            mtu: None,
            mtu_before: interface.mtu(),
            tags_added: vec![InterfaceTag::Dhcp],
            tags_removed: Vec::new(),
            poe: None,
            poe_before: None,
        }));
    }
    Ok(plan)
}

/// The virtual interface of `vlan` on `device`: untagged in it, no port or bridge.
fn vlan_interface(device: &DeviceAccess, vlan: &VlanAccess) -> Option<InterfaceAccess> {
    device.interfaces().into_iter().find(|interface| {
        !interface.is_ethernet_port()
            && !interface.is_bridge()
            && interface.radio().is_none()
            && interface.untagged_vlan().is_some_and(|v| v.id == vlan.id)
    })
}

fn ipv4(interface: &InterfaceAccess) -> Option<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V4(_)))
}

/// How many free networks are looked at, as `connection::CANDIDATES`.
const CANDIDATES: usize = 32;

/// The first network of `length` in `parent` that NetBox has free (`available-prefixes`, no
/// addresses in it) and the network doesn't route; the routed ones go into `plan.undocumented`.
async fn next_free(
    rest: &NetboxRest,
    routes: &Routes,
    plan: &mut NetboxPlan,
    id: u32,
    parent: IpNet,
    length: u8,
) -> Result<IpNet, Error> {
    let free = rest
        .get(&format!("ipam/prefixes/{id}/available-prefixes/"))
        .await?;
    let mut candidates = Vec::new();
    for block in free.as_array().into_iter().flatten() {
        if let Some(Ok(block)) = block["prefix"].as_str().map(str::parse::<IpNet>)
            && let Ok(subnets) = block.subnets(length)
        {
            candidates.extend(subnets.take(CANDIDATES));
        }
    }
    for net in candidates.into_iter().take(CANDIDATES) {
        if let Some(taken) = routes.overlapping(net, parent) {
            if !plan.undocumented.iter().any(|u| u.route == taken.route) {
                plan.undocumented.push(taken);
            }
            continue;
        }
        let addresses = rest
            .get(&format!("ipam/ip-addresses/?parent={net}&limit=1"))
            .await?;
        if addresses["count"].as_u64() == Some(0) {
            return Ok(net);
        }
    }
    Err(Error::NoFreeNetwork {
        parent: parent.to_string().into(),
        length,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::device::ros::netbox_example_test::example_topology;

    /// A network on a VLAN without interface: the preview has the new virtual interface in the
    /// VLAN with the gateway and `dhcp`, its prefix scoped to the site.
    #[test]
    fn a_vlan_gets_its_interface_with_the_gateway() {
        let topology = example_topology();
        let (device, vlan) = topology
            .list_devices()
            .filter(|d| d.has_routeros() && d.site().is_some())
            .find_map(|device| {
                let vlan = topology.list_vlans().find(|vlan| {
                    vlan.devices().iter().any(|d| d.id() == device.id())
                        && vlan_interface(&device, vlan).is_none()
                })?;
                Some((device, vlan))
            })
            .expect("a VLAN without interface");
        let site = device.site().unwrap().id();
        let name: Box<str> = vlan.name().unwrap().into();
        let mut plan = NetboxPlan::new(&device, "test");
        plan.steps = vec![
            PlanStep::CreateInterface(CreateInterface {
                device: device_ref(&device),
                name: name.clone(),
                kind: "virtual".into(),
                vlan: Some(vlan.id.0),
                tags: vec![InterfaceTag::Dhcp],
            }),
            PlanStep::CreatePrefix(CreatePrefix {
                prefix: "198.51.100.0/24".into(),
                pool: "198.51.100.0/22".into(),
                description: "test".into(),
                site: Some(site.0),
                vlan: Some(vlan.id.0),
            }),
            PlanStep::CreateAddress(CreateAddress {
                address: "198.51.100.1/24".into(),
                device: device_ref(&device),
                interface: name.clone(),
                prefix: "198.51.100.0/24".into(),
                on: Target::Step(0),
            }),
        ];
        let after = plan.preview_topology().expect("preview");
        let device_after = after.get_device_by_id(&device.id()).unwrap();
        let vlan_after = after.list_vlans().find(|v| v.id == vlan.id).unwrap();
        let interface = vlan_interface(&device_after, &vlan_after).expect("the new interface");
        assert_eq!(interface.name(), &*name);
        assert!(interface.is_enable_dhcp_server());
        let address = ipv4(&interface).expect("the gateway");
        assert_eq!(address.to_string(), "198.51.100.1/24");
        let prefix = interface.ips()[0].prefix().and_then(|p| p.data().cloned()).unwrap();
        assert_eq!(prefix.site, Some(site));
    }

    /// The choices leave out ports with an address, links to routers and bridge ports, and
    /// offer only prefixes of the device's site that aren't a network themselves.
    #[test]
    fn options_are_free_ports_and_site_prefixes() {
        let topology = example_topology();
        for device in topology.list_devices().filter(|d| d.has_routeros()) {
            let options = options(&device);
            for choice in &options.interfaces {
                let interface = topology.get_interface_by_id(InterfaceId(choice.id)).unwrap();
                assert!(ipv4(&interface).is_none() && interface.bridge().is_none(), "{choice:?}");
            }
            let site = device.site().map(|s| s.id());
            for parent in &options.parents {
                let data = topology.prefix_data(IpPrefixId(parent.id)).unwrap();
                assert_eq!(data.site, site);
                assert!(data.addresses.is_empty() && data.pool.is_none());
            }
        }
    }
}
