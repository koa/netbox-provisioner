//! Scope: The helper „Kunden-Netz“; nothing else.
//!
//! Helper „Kunden-Netz“: a network for a port or a VLAN of a device, out of a prefix of its
//! site chosen by hand (`docs/config-helper.md`). The next free network of the chosen
//! length in it, scoped to the site, its first address as gateway on the port or on a virtual
//! interface of the VLAN (a new one named like the VLAN, `docs/netbox-modeling.md`), and the
//! tag `dhcp` if wanted. Without an IP range the DHCP pool is the rest of the network; one
//! with the role `dhcp` (E6) may come with it, the upper half of the network unless chosen.
//!
//! IPv6 likewise, out of an IPv6 prefix of the site chosen by hand: the next free `/64`, its
//! first address as gateway on the same interface, which `dhcp` announces for SLAAC. Either
//! family may come alone, so a network that has IPv4 already gets its IPv6 later.
//!
//! As with the links, a network NetBox has free may be in use: one the network routes in part
//! (`routes::Routes::overlapping`) or with addresses in NetBox is passed over.
use super::{
    CreateAddress, CreateInterface, CreateIpRange, CreatePrefix, NetboxPlan, PlanStep, Target,
    UpdateInterface, device_ref, flags::InterfaceTag, routes::Routes,
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
use ipnet::{IpNet, Ipv4Net};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

/// Where the network goes.
#[derive(Debug, Clone, PartialEq, Eq, OneofObject)]
pub enum CustomerNetTarget {
    /// a port or bridge of the device without IPv4, or a client network (`dhcp`) without IPv6
    Interface(u32),
    /// a VLAN the device has: its virtual interface, or a new one
    Vlan(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct CustomerNetInput {
    pub device: u32,
    pub target: CustomerNetTarget,
    /// the IPv4 prefix of the site the network is taken from
    pub parent: Option<u32>,
    /// the IPv4 network's length, `/24` without
    pub length: Option<u8>,
    /// the IPv6 prefix of the site its `/64` is taken from
    pub parent_v6: Option<u32>,
    /// the tag `dhcp`: a DHCP server on the network
    pub dhcp: bool,
    /// an IP range with the role `dhcp` in the IPv4 network, the server's pool
    pub dhcp_range: Option<DhcpRangeInput>,
}

/// The first and last address of a DHCP range as numbers in the network (`.128` of a `/24` is
/// 128), none for the upper half's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, InputObject)]
pub struct DhcpRangeInput {
    pub first: Option<u32>,
    pub last: Option<u32>,
}

/// What „Kunden-Netz“ can choose from for a device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetOptions {
    pub interfaces: Box<[CustomerNetInterface]>,
    pub vlans: Box<[CustomerNetVlan]>,
    pub parents: Box<[CustomerNetParent]>,
    pub parents_v6: Box<[CustomerNetParent]>,
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
/// The length of an IPv6 client network: SLAAC needs a `/64`.
const LENGTH_V6: u8 = 64;

/// Whether `interface` (a port, bridge or VLAN interface) can take a network: without IPv4,
/// or a client network (`dhcp`) without IPv6.
fn open(interface: &InterfaceAccess) -> bool {
    ipv4(interface).is_none() || (interface.is_enable_dhcp_server() && ipv6(interface).is_none())
}

/// The ports and bridges that can take a network ([`open`]) and aren't a link to a router or
/// in a bridge or VLAN, the VLANs of the device that can, and the prefixes of its site.
pub fn options(device: &DeviceAccess) -> CustomerNetOptions {
    let interfaces = device.interfaces();
    let mut free: Vec<CustomerNetInterface> = interfaces
        .iter()
        .filter(|interface| {
            (interface.is_ethernet_port() || interface.is_bridge())
                && open(interface)
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
            if gateway.as_ref().is_some_and(|i| !open(i)) {
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
    let parents = |v6| {
        device
            .site()
            .map(|site| device.topology().customer_parents(site.id(), v6))
            .unwrap_or_default()
            .into_iter()
            .map(|parent| CustomerNetParent {
                id: parent.id.0,
                prefix: parent.net.to_string().into(),
            })
            .collect()
    };
    CustomerNetOptions {
        interfaces: free.into(),
        vlans: vlans.into(),
        parents: parents(false),
        parents_v6: parents(true),
    }
}

pub async fn plan(topology: &Arc<Topology>, input: &CustomerNetInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let site = device
        .site()
        .ok_or_else(|| Error::NoSite(device.name().into()))?;
    let parent_of = |id: u32, v6: bool| {
        topology
            .prefix_data(IpPrefixId(id))
            .filter(|parent| parent.site == Some(site.id()))
            .map(|parent| parent.prefix)
            .filter(|prefix| matches!(prefix, IpNet::V6(_)) == v6)
            .ok_or(Error::UnknownPrefix(id))
    };
    let parent = input.parent.map(|id| parent_of(id, false)).transpose()?;
    let parent_v6 = input.parent_v6.map(|id| parent_of(id, true)).transpose()?;
    if input.dhcp_range.is_some() && (parent.is_none() || !input.dhcp) {
        return Err(Error::DhcpRangeWithoutDhcp);
    }
    if parent.is_none() && parent_v6.is_none() {
        return Err(Error::NoParent);
    }
    let length = input.length.unwrap_or(DEFAULT_LENGTH);
    if let Some(parent) = parent
        && (length <= parent.prefix_len() || length > 30)
    {
        return Err(Error::NetworkLength {
            length,
            parent: parent.to_string().into(),
        });
    }
    if let Some(parent) = parent_v6
        && LENGTH_V6 <= parent.prefix_len()
    {
        return Err(Error::NetworkLength {
            length: LENGTH_V6,
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
    // an address of the family a network is wanted for: the port has its network
    for (wanted, family) in [
        (parent.is_some(), ipv4 as fn(&_) -> _),
        (parent_v6.is_some(), ipv6),
    ] {
        if let Some(interface) = existing.as_ref().filter(|_| wanted)
            && let Some(address) = family(interface)
        {
            return Err(Error::PortAddressed {
                port: format!("{} {}", device.name(), interface.name()).into(),
                address: address.to_string().into(),
            });
        }
    }
    let mut plan = NetboxPlan::new(&device, format!("Kunden-Netz {what}"));
    let rest = NetboxRest::reader()?;
    let routes = Routes::read(topology).await?;
    plan.checked_with = Some(routes.router.clone());
    let mut nets = Vec::new();
    for (id, parent, length) in [
        (input.parent, parent, length),
        (input.parent_v6, parent_v6, LENGTH_V6),
    ] {
        if let (Some(id), Some(parent)) = (id, parent) {
            let net = next_free(&rest, &routes, &mut plan, id, parent, length).await?;
            nets.push((parent, net));
        }
    }

    let dhcp = input.dhcp.then_some(InterfaceTag::Dhcp);
    let (on, interface_name) = match &existing {
        Some(interface) => (
            Target::Existing(interface.id().0),
            Box::from(interface.name()),
        ),
        None => {
            let vlan = vlan.as_ref().expect("a VLAN without its interface");
            let name: Box<str> = vlan.name().unwrap_or_default().into();
            plan.steps.push(PlanStep::CreateInterface(CreateInterface {
                device: device_ref(&device),
                name: name.clone(),
                kind: "virtual".into(),
                vlan: Some(vlan.id.0),
                tags: dhcp.into_iter().collect(),
                mac: None,
            }));
            (Target::Step(plan.steps.len() - 1), name)
        }
    };
    for (parent, net) in nets {
        plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
            prefix: net.to_string().into(),
            pool: parent.to_string().into(),
            description: what.clone().into(),
            site: Some(site.id().0),
            vlan: vlan.as_ref().map(|vlan| vlan.id.0),
            dhcp_lease_time: None,
        }));
        plan.steps.push(PlanStep::CreateAddress(CreateAddress {
            address: gateway(net).to_string().into(),
            device: device_ref(&device),
            interface: interface_name.clone(),
            prefix: net.to_string().into(),
            on,
        }));
        if let (IpNet::V4(net), Some(range)) = (net, input.dhcp_range) {
            plan.steps
                .push(PlanStep::CreateIpRange(dhcp_range(net, range, &what)?));
        }
    }
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
            tags_added: vec![InterfaceTag::Dhcp].into(),
            tags_removed: Vec::new(),
            poe: None,
            poe_before: None,
            poe_removed: false,
        }));
    }
    Ok(plan)
}

/// The DHCP range `range` asks for in `net`: from the address after the gateway to the one
/// before the broadcast, by default the upper half.
fn dhcp_range(
    net: Ipv4Net,
    range: DhcpRangeInput,
    description: &str,
) -> Result<CreateIpRange, Error> {
    let size = 1u32 << (32 - net.prefix_len());
    let first = range.first.unwrap_or(size / 2);
    let last = range.last.unwrap_or(size - 2);
    if first < 2 || last < first || last > size - 2 {
        return Err(Error::DhcpRangeOutside {
            first,
            last,
            length: net.prefix_len(),
        });
    }
    let address = |n: u32| {
        let ip = Ipv4Addr::from(u32::from(net.network()) + n);
        format!("{ip}/{}", net.prefix_len()).into()
    };
    Ok(CreateIpRange {
        start: address(first),
        end: address(last),
        description: description.into(),
    })
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

/// The first address of `net` (`::1` for IPv6, whose network address is the subnet-router
/// anycast), with its length.
fn gateway(net: IpNet) -> IpNet {
    let first = match net {
        IpNet::V4(net) => IpAddr::V4((u32::from(net.network()) + 1).into()),
        IpNet::V6(net) => IpAddr::V6((u128::from(net.network()) + 1).into()),
    };
    IpNet::new(first, net.prefix_len()).expect("the length of the network")
}

fn ipv4(interface: &InterfaceAccess) -> Option<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V4(_)))
}

/// A global IPv6 address of `interface` (not link-local).
fn ipv6(interface: &InterfaceAccess) -> Option<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V6(v6) if !v6.addr().is_unicast_link_local()))
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
            .find(
                "ipam/ip-addresses/",
                &[("parent", &net.to_string()), ("limit", "1")],
            )
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
mod test;
