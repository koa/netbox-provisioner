//! Helper „Kunden-Netz“: a network for a port or a VLAN of a device, out of a prefix of its
//! site chosen by hand (`docs/concepts/config-helper.md`). The next free network of the chosen
//! length in it, scoped to the site, its first address as gateway on the port or on a virtual
//! interface of the VLAN (a new one named like the VLAN, `docs/netbox-modeling.md`), and the
//! tag `dhcp` if wanted. Without an IP range the DHCP pool is the rest of the network.
//!
//! IPv6 likewise, out of an IPv6 prefix of the site chosen by hand: the next free `/64`, its
//! first address as gateway on the same interface, which `dhcp` announces for SLAAC. Either
//! family may come alone, so a network that has IPv4 already gets its IPv6 later.
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
use std::{net::IpAddr, sync::Arc};

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
}

/// What „Kunden-Netz“ can choose from for a device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct CustomerNetOptions {
    pub interfaces: Vec<CustomerNetInterface>,
    pub vlans: Vec<CustomerNetVlan>,
    pub parents: Vec<CustomerNetParent>,
    pub parents_v6: Vec<CustomerNetParent>,
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
            .map(|(id, prefix)| CustomerNetParent {
                id: id.0,
                prefix: prefix.to_string().into(),
            })
            .collect()
    };
    CustomerNetOptions {
        interfaces: free,
        vlans,
        parents: parents(false),
        parents_v6: parents(true),
    }
}

pub async fn plan(topology: &Arc<Topology>, input: &CustomerNetInput) -> Result<NetboxPlan, Error> {
    let device = topology
        .get_device_by_id(&DeviceId(input.device))
        .ok_or(Error::UnknownDevice(input.device))?;
    let site = device.site().ok_or_else(|| Error::NoSite(device.name().into()))?;
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
    for (wanted, family) in [(parent.is_some(), ipv4 as fn(&_) -> _), (parent_v6.is_some(), ipv6)] {
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
        }));
        plan.steps.push(PlanStep::CreateAddress(CreateAddress {
            address: gateway(net).to_string().into(),
            device: device_ref(&device),
            interface: interface_name.clone(),
            prefix: net.to_string().into(),
            on,
        }));
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
            tags_added: vec![InterfaceTag::Dhcp],
            tags_removed: Vec::new(),
            poe: None,
            poe_before: None,
            poe_removed: false,
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
    use crate::topology::test::example_topology;

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
                mac: None,
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

    /// The choices leave out ports with their networks, links to routers and bridge ports, and
    /// offer only prefixes of the device's site that aren't a network themselves, by family.
    #[test]
    fn options_are_free_ports_and_site_prefixes() {
        let topology = example_topology();
        for device in topology.list_devices().filter(|d| d.has_routeros()) {
            let options = options(&device);
            for choice in &options.interfaces {
                let interface = topology.get_interface_by_id(InterfaceId(choice.id)).unwrap();
                assert!(open(&interface) && interface.bridge().is_none(), "{choice:?}");
            }
            let site = device.site().map(|s| s.id());
            for (parents, v6) in [(&options.parents, false), (&options.parents_v6, true)] {
                for parent in parents {
                    let data = topology.prefix_data(IpPrefixId(parent.id)).unwrap();
                    assert_eq!(data.site, site);
                    assert!(data.addresses.is_empty() && data.pool.is_none());
                    assert_eq!(matches!(data.prefix, IpNet::V6(_)), v6, "{parent:?}");
                }
            }
        }
    }

    /// The gateway is the first address: `.1` of an IPv4 network, `::1` of a `/64`.
    #[test]
    fn the_gateway_is_the_first_address() {
        let net = |net: &str| gateway(net.parse().unwrap()).to_string();
        assert_eq!(net("192.0.2.0/24"), "192.0.2.1/24");
        assert_eq!(net("2001:db8:0:7::/64"), "2001:db8:0:7::1/64");
    }

    /// A client network without IPv6 is open for one; one with both, or IPv4 without `dhcp`
    /// (an uplink), isn't.
    #[test]
    fn a_client_network_without_ipv6_is_open() {
        let topology = example_topology();
        let client = |v6: bool| {
            topology.list_devices().flat_map(|d| d.interfaces()).find(|interface| {
                interface.is_enable_dhcp_server()
                    && ipv4(interface).is_some()
                    && ipv6(interface).is_some() == v6
            })
        };
        if let Some(interface) = client(false) {
            assert!(open(&interface), "{}", interface.name());
        }
        if let Some(interface) = client(true) {
            assert!(!open(&interface), "{}", interface.name());
        }
        let uplink = topology
            .list_devices()
            .flat_map(|d| d.interfaces())
            .find(|interface| !interface.is_enable_dhcp_server() && ipv4(interface).is_some())
            .expect("an address without dhcp");
        assert!(!open(&uplink), "{}", uplink.name());
    }
}
