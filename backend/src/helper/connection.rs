//! Helper „Verbindung“, direct: two router ports become a link of the OSPF network. The cable
//! between them if NetBox lacks it, the next free `/31` of the transfer pool with one address
//! at each end (RFC 3021: no network or broadcast address), `ospf` at both ends, the same MTU at
//! both and as label the name of the device at the other end (`docs/netbox-modeling.md`).
//! What is there already stays: a cable between the two, addresses in a common network.
//!
//! As with the loopbacks, a `/31` NetBox has free may be in use by devices it doesn't know of:
//! one the network routes (`routes::Routes`) or with addresses in NetBox is passed over.
use super::{
    CreateAddress, CreateCable, CreatePrefix, NetboxPlan, PlanStep, Target, UpdateInterface,
    device_ref, flags::InterfaceTag, port_name, routes::Routes,
};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{
        InterfaceId, PrefixPool, Topology,
        access::{device::DeviceAccess, interface::InterfaceAccess},
        findings::{InterfaceRef, interface_ref},
        natural::naturalize_interface,
    },
};
use async_graphql::{InputObject, SimpleObject};
use ipnet::IpNet;
use std::sync::Arc;

/// The two ends of a link between routers.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct ConnectionInput {
    /// the port on the device the plan is for
    pub interface: u32,
    /// the port at the other end
    pub peer: u32,
    /// the L3 MTU of both ends; without it the one an end has (the larger one if both have one)
    pub mtu: Option<u16>,
}

pub async fn plan(topology: &Arc<Topology>, input: &ConnectionInput) -> Result<NetboxPlan, Error> {
    let end = |id| {
        topology
            .get_interface_by_id(InterfaceId(id))
            .ok_or(Error::UnknownInterface(id))
    };
    let (a, b) = (end(input.interface)?, end(input.peer)?);
    let (Some(a_device), Some(b_device)) = (a.device(), b.device()) else {
        return Err(Error::UnknownInterface(input.interface));
    };
    if a_device.id() == b_device.id() {
        return Err(Error::SameDevice(a_device.name().into()));
    }
    for device in [&a_device, &b_device] {
        if !device.has_routeros() {
            return Err(Error::NoRouterOs(device.name().into()));
        }
    }
    let mut plan = NetboxPlan::new(
        &a_device,
        format!(
            "Verbindung {} {} – {} {}",
            a_device.name(),
            a.name(),
            b_device.name(),
            b.name()
        ),
    );

    if !a.connected_interfaces().iter().any(|i| i.id() == b.id()) {
        for end in [&a, &b] {
            if end.cable().is_some() {
                return Err(Error::PortCabled {
                    port: port_name(end),
                    far: end.connected_interfaces().first().map(port_name),
                });
            }
        }
        plan.steps.push(PlanStep::CreateCable(CreateCable {
            a: interface_ref(&a),
            b: interface_ref(&b),
            a_end: super::CableEnd::Interface,
        }));
    }
    for end in [&a, &b] {
        if let Some(bridge) = end.bridge() {
            return Err(Error::PortInBridge {
                port: port_name(end),
                bridge: bridge.name().into(),
            });
        }
    }

    match (ipv4(&a), ipv4(&b)) {
        (Some(x), Some(y)) if x.trunc() == y.trunc() => {}
        (None, None) => {
            let rest = NetboxRest::reader()?;
            let routes = Routes::read(topology).await?;
            plan.checked_with = Some(routes.router.clone());
            let (pool, net) = next_free(topology, &rest, &routes, &mut plan).await?;
            plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
                prefix: net.to_string().into(),
                pool: pool.to_string().into(),
                description: format!("{} – {}", a_device.name(), b_device.name()).into(),
                site: None,
                vlan: None,
                dhcp_lease_time: None,
            }));
            for (end, address) in [(&a, net.network()), (&b, net.broadcast())] {
                plan.steps.push(PlanStep::CreateAddress(CreateAddress {
                    device: end.device().as_ref().map(device_ref).expect("checked above"),
                    address: IpNet::new(address, 31)
                        .expect("a /31")
                        .to_string()
                        .into(),
                    interface: end.name().into(),
                    prefix: net.to_string().into(),
                    on: Target::Existing(end.id().0),
                }));
            }
        }
        (Some(address), _) => {
            return Err(Error::PortAddressed {
                port: port_name(&a),
                address: address.to_string().into(),
            });
        }
        (None, Some(address)) => {
            return Err(Error::PortAddressed {
                port: port_name(&b),
                address: address.to_string().into(),
            });
        }
    }

    // None is less than Some: the larger one an end has
    let mtu = input.mtu.or(a.mtu().max(b.mtu()));
    for (end, far) in [(&a, &b_device), (&b, &a_device)] {
        let label = (end.label() != Some(far.name())).then(|| far.name().into());
        let new_mtu = mtu.filter(|mtu| end.mtu() != Some(*mtu));
        let ospf = !end.use_ospf();
        if label.is_some() || new_mtu.is_some() || ospf {
            plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
                interface: interface_ref(end),
                label,
                label_before: end.label().map(Box::from),
                mtu: new_mtu,
                mtu_before: end.mtu(),
                tags_added: ospf.then_some(InterfaceTag::Ospf).into_iter().collect(),
                tags_removed: Vec::new(),
                poe: None,
                poe_before: None,
                poe_removed: false,
            }));
        }
    }
    Ok(plan)
}

/// A physical port of a device and what it has that matters for a link.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LinkPort {
    pub id: u32,
    pub name: Box<str>,
    pub label: Option<Box<str>>,
    pub mtu: Option<u16>,
    pub ospf: bool,
    /// its IPv4 address, with the length
    pub ipv4: Option<Box<str>>,
    /// the bridge it is a port of
    pub bridge: Option<Box<str>>,
    /// whether it has a cable, and the interfaces it reaches
    pub cabled: bool,
    pub connected: Vec<InterfaceRef>,
}

/// The device's Ethernet ports (SFP included, no `lo` or radio: a direct link is a cable) in
/// NetBox' order.
pub fn link_ports(device: &DeviceAccess) -> Vec<LinkPort> {
    let mut ports: Vec<LinkPort> = device
        .interfaces()
        .iter()
        .filter(|interface| interface.is_ethernet_port())
        .map(|interface| LinkPort {
            id: interface.id().0,
            name: interface.name().into(),
            label: interface.label().map(Box::from),
            mtu: interface.mtu(),
            ospf: interface.use_ospf(),
            ipv4: ipv4(interface).map(|net| net.to_string().into()),
            bridge: interface.bridge().map(|bridge| bridge.name().into()),
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

fn ipv4(interface: &InterfaceAccess) -> Option<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V4(_)))
}

/// How many free `/31` of the pool are looked at, as `loopback::CANDIDATES`.
const CANDIDATES: usize = 32;

/// The first `/31` of the transfer pool that NetBox has free (`available-prefixes`, no
/// addresses in it either: an older link may have its addresses without a prefix) and the
/// network doesn't route, and the pool prefix it is in. The routed ones passed over go into
/// `plan.undocumented`.
async fn next_free(
    topology: &Arc<Topology>,
    rest: &NetboxRest,
    routes: &Routes,
    plan: &mut NetboxPlan,
) -> Result<(IpNet, IpNet), Error> {
    let mut looked_at = 0;
    for (id, pool) in topology.pool_prefixes(PrefixPool::Transfer, false) {
        let free = rest
            .get(&format!("ipam/prefixes/{}/available-prefixes/", id.0))
            .await?;
        let mut candidates = Vec::new();
        for block in free.as_array().into_iter().flatten() {
            if let Some(Ok(block)) = block["prefix"].as_str().map(str::parse::<IpNet>)
                && let Ok(subnets) = block.subnets(31)
            {
                candidates.extend(subnets.take(CANDIDATES));
            }
        }
        for net in candidates {
            looked_at += 1;
            if looked_at > CANDIDATES {
                return Err(Error::NoFreeAddress("transfer", 4));
            }
            let taken = [net.network(), net.broadcast()]
                .into_iter()
                .filter_map(|host| routes.in_use(host, pool))
                .collect::<Vec<_>>();
            if !taken.is_empty() {
                for taken in taken {
                    if !plan.undocumented.iter().any(|u| u.route == taken.route) {
                        plan.undocumented.push(taken);
                    }
                }
                continue;
            }
            let addresses = rest
                .get(&format!("ipam/ip-addresses/?parent={net}&limit=1"))
                .await?;
            if addresses["count"].as_u64() == Some(0) {
                return Ok((pool, net));
            }
        }
    }
    Err(Error::NoFreeAddress("transfer", 4))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::topology::test::example_topology;

    /// A link NetBox has whole, cable and common network, gets no cable, prefix or address,
    /// only what its ends lack; asking for it from either end is the same.
    #[tokio::test]
    async fn an_existing_link_gets_only_what_it_lacks() {
        let topology = example_topology();
        let (a, b) = topology
            .list_devices()
            .filter(|d| d.has_routeros())
            .flat_map(|d| d.interfaces().into_vec())
            .find_map(|a| {
                let b = a.connected_interfaces().first()?.clone();
                let routers = [&a, &b]
                    .iter()
                    .all(|end| end.device().is_some_and(|d| d.has_routeros()));
                let common = ipv4(&a)?.trunc() == ipv4(&b)?.trunc();
                (routers && common && a.bridge().is_none() && b.bridge().is_none())
                    .then_some((a, b))
            })
            .expect("a link between routers");
        let input = ConnectionInput {
            interface: a.id().0,
            peer: b.id().0,
            mtu: None,
        };
        let plan = plan(&topology, &input).await.expect("plan");
        assert!(
            plan.steps
                .iter()
                .all(|step| matches!(step, PlanStep::UpdateInterface(_))),
            "{:?}",
            plan.steps
        );
        assert!(plan.checked_with.is_none(), "no address, no route check");
        let mtu = a.mtu().max(b.mtu());
        for step in &plan.steps {
            let PlanStep::UpdateInterface(update) = step else { unreachable!() };
            let end = if update.interface.id == a.id().0 { &a } else { &b };
            assert_eq!(update.mtu.is_some(), end.mtu() != mtu);
            assert_eq!(update.tags_added.contains(&InterfaceTag::Ospf), !end.use_ospf());
        }
        let reverse = ConnectionInput {
            interface: b.id().0,
            peer: a.id().0,
            mtu: None,
        };
        let reversed = super::plan(&topology, &reverse).await.expect("plan");
        assert_eq!(reversed.steps.len(), plan.steps.len());
    }

    /// A port cabled to something else is refused, naming where its cable goes.
    #[tokio::test]
    async fn a_port_cabled_elsewhere_is_refused() {
        let topology = example_topology();
        let routers = || topology.list_devices().filter(|d| d.has_routeros());
        let cabled = routers()
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| !i.connected_interfaces().is_empty())
            .expect("a cabled port");
        let other = routers()
            .filter(|d| Some(d.id()) != cabled.device().map(|d| d.id()))
            .flat_map(|d| d.interfaces().into_vec())
            .find(|i| {
                i.is_ethernet_port()
                    && i.cable().is_none()
                    && !cabled.connected_interfaces().iter().any(|c| c.id() == i.id())
            })
            .expect("a free port");
        let input = ConnectionInput {
            interface: cabled.id().0,
            peer: other.id().0,
            mtu: None,
        };
        match plan(&topology, &input).await {
            Err(Error::PortCabled { far: Some(_), .. }) => {}
            other => panic!("{:?}", other.map(|plan| plan.steps)),
        }
    }
}
