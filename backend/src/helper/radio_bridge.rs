//! Helper „Funkbrücke“: two radio bridge devices between two routers, each bridging its
//! Ethernet port and its radio, become a link NetBox describes whole
//! (`docs/radio-bridge-generation.md`). The forwarding stays L2; the bridges are OSPF routers
//! for their own management only:
//!
//! - the cable from each router port to the bridge's Ethernet port, where it lacks;
//! - on each bridge an interface `bridge` with the Ethernet port and the radio as its ports,
//!   the radio named as RouterOS names it (`wlan60-<N>` for 802.11ad/ay), RF role AP or
//!   station, `lo` with its loopback (as the helper „Loopback“);
//! - the wireless link between the radios, with an SSID and a PSK (one is made where the link
//!   has none);
//! - one IPv4 network for the segment, the one a router port has, else a new one of the
//!   transfer pool: each router port and each `bridge` an address in it, all with `ospf`. IPv6
//!   needs no address: OSPFv3 runs over the link-local ones, the bridges are reached at their
//!   loopbacks.
//!
//! What is there already stays; what is missing or doesn't fit becomes a step of the plan.
//! Something that doesn't fit and isn't the helper's to change (a cable or wireless link to
//! another device, an address in another network) stops it with an error naming it.
use super::{
    CreateAddress, CreateCable, CreateInterface, CreatePrefix, CreateWirelessLink, NetboxPlan,
    PlanStep, Target, UpdateInterface, UpdatePort, UpdateWirelessLink, device_ref,
    flags::InterfaceTag, loopback, new_psk, port_name, routes::Routes,
};
use crate::{
    Error,
    netbox::rest::NetboxRest,
    topology::{
        DeviceId, PhysicalPortId, PortType, PrefixPool, RadioRole, Topology,
        access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess},
        findings::{DeviceRef, InterfaceRef, interface_ref},
    },
};
use async_graphql::{InputObject, SimpleObject};
use ipnet::{IpNet, Ipv4Net};
use std::{net::IpAddr, sync::Arc};

/// The two radio bridge devices of a link and what the form says about them.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct RadioBridgeInput {
    /// the bridge device whose radio is AP (the plan is for it)
    pub ap: u32,
    /// the bridge device whose radio is station
    pub station: u32,
    /// the router port at the AP's bridge; without it the one its Ethernet port is cabled to
    pub ap_router_port: Option<u32>,
    /// the router port at the station's bridge, as `ap_router_port`
    pub station_router_port: Option<u32>,
    /// the IPv4 address of the AP's `bridge` with its length; without it the one it has or the
    /// next free one of the network
    pub ap_address: Option<String>,
    /// the IPv4 address of the station's `bridge`, as `ap_address`
    pub station_address: Option<String>,
    /// without it the wireless link's, else the AP device's name
    pub ssid: Option<String>,
}

/// One bridge device of the link and what the plan found of it.
struct Side {
    device: DeviceAccess,
    radio: InterfaceAccess,
    router_port: InterfaceAccess,
    /// the interface `bridge`: an existing one or the step creating it
    bridge: Target,
    bridge_name: Box<str>,
    address: Option<String>,
}

/// The ends of the segment: router port (or its bridge) and the bridges, by NetBox interface.
struct End {
    device: DeviceAccess,
    interface: Option<InterfaceAccess>,
    on: Target,
    name: Box<str>,
    wanted: Option<String>,
}

pub async fn plan(topology: &Arc<Topology>, input: &RadioBridgeInput) -> Result<NetboxPlan, Error> {
    let mut routes = None;
    plan_with(topology, input, &mut routes).await
}

/// The plan, with `routes` read once an address is needed (tests give them).
pub(crate) async fn plan_with(
    topology: &Arc<Topology>,
    input: &RadioBridgeInput,
    routes: &mut Option<Routes>,
) -> Result<NetboxPlan, Error> {
    let (mut plan, sides) = link_plan(topology, input)?;
    for side in &sides {
        loopback::add_to(&mut plan, &side.device, routes).await?;
    }
    segment(topology, &mut plan, &sides, routes).await?;
    Ok(plan)
}

/// The steps for cables, bridges and radios, which need nothing but NetBox as the topology has
/// it, and the two sides they found.
fn link_plan(
    topology: &Arc<Topology>,
    input: &RadioBridgeInput,
) -> Result<(NetboxPlan, Vec<Side>), Error> {
    let device = |id| {
        topology
            .get_device_by_id(&DeviceId(id))
            .ok_or(Error::UnknownDevice(id))
    };
    let (ap, station) = (device(input.ap)?, device(input.station)?);
    if ap.id() == station.id() {
        return Err(Error::SameDevice(ap.name().into()));
    }
    let mut plan = NetboxPlan::new(
        &ap,
        format!("Funkbrücke {} – {}", ap.name(), station.name()),
    );
    let mut sides = Vec::new();
    for (device, role, router_port, address, other) in [
        (&ap, RadioRole::Ap, input.ap_router_port, &input.ap_address, &station),
        (
            &station,
            RadioRole::Station,
            input.station_router_port,
            &input.station_address,
            &ap,
        ),
    ] {
        sides.push(side(topology, &mut plan, device, role, router_port, address, other)?);
    }
    radios(&mut plan, &sides, input.ssid.as_deref())?;
    Ok((plan, sides))
}

/// The radio, the Ethernet port, the router port and the bridge of `device`, with the steps for
/// the cable and the bridge's ports.
fn side(
    topology: &Arc<Topology>,
    plan: &mut NetboxPlan,
    device: &DeviceAccess,
    role: RadioRole,
    router_port: Option<u32>,
    address: &Option<String>,
    other: &DeviceAccess,
) -> Result<Side, Error> {
    if !device.has_routeros() {
        return Err(Error::NoRouterOs(device.name().into()));
    }
    let interfaces = device.interfaces();
    let radios = interfaces.iter().filter(|i| is_radio(i)).collect::<Vec<_>>();
    let [radio] = radios[..] else {
        return Err(Error::NoSingleRadio(device.name().into()));
    };
    let ethernet = || interfaces.iter().filter(|i| i.is_ethernet_port());
    let to_router = |port: &InterfaceAccess| {
        port.connected_interfaces().into_iter().find(|far| {
            far.device()
                .is_some_and(|d| d.has_routeros() && d.id() != other.id() && d.id() != device.id())
        })
    };
    let (ether, router_port) = match router_port {
        Some(id) => {
            let router_port = topology
                .get_interface_by_id(crate::topology::InterfaceId(id))
                .ok_or(Error::UnknownInterface(id))?;
            let router = router_port.device().ok_or(Error::UnknownInterface(id))?;
            if !router.has_routeros() {
                return Err(Error::NoRouterOs(router.name().into()));
            }
            let cabled = ethernet().find(|port| {
                port.connected_interfaces().iter().any(|far| far.id() == router_port.id())
            });
            let ether = match cabled {
                Some(port) => port.clone(),
                None => {
                    let free = ethernet()
                        .find(|port| port.cable().is_none())
                        .ok_or_else(|| Error::NoRouterPort(device.name().into()))?;
                    if router_port.cable().is_some() {
                        return Err(Error::PortCabled {
                            port: port_name(&router_port),
                            far: router_port.connected_interfaces().first().map(port_name),
                        });
                    }
                    plan.steps.push(PlanStep::CreateCable(CreateCable {
                        a: interface_ref(&router_port),
                        b: interface_ref(free),
                    }));
                    free.clone()
                }
            };
            (ether, router_port)
        }
        None => ethernet()
            .find_map(|port| to_router(port).map(|far| (port.clone(), far)))
            .ok_or_else(|| Error::NoRouterPort(device.name().into()))?,
    };

    let existing = interfaces
        .iter()
        .filter(|i| i.data().and_then(|d| d.port_type) == Some(PortType::Bridge))
        .collect::<Vec<_>>();
    let found = radio
        .bridge()
        .filter(|bridge| existing.iter().any(|e| e.id() == bridge.id()))
        .or_else(|| existing.first().map(|e| (*e).clone()));
    let (bridge, bridge_name): (Target, Box<str>) = match found {
        Some(bridge) => (Target::Existing(bridge.id().0), Box::from(bridge.name())),
        None => {
            if interfaces.iter().any(|i| i.name() == "bridge") {
                return Err(Error::NameTaken {
                    device: device.name().into(),
                    name: "bridge".into(),
                });
            }
            plan.steps.push(PlanStep::CreateInterface(CreateInterface {
                device: device_ref(device),
                name: "bridge".into(),
                kind: "bridge".into(),
                vlan: None,
                tags: vec![InterfaceTag::Ospf],
            }));
            (Target::Step(plan.steps.len() - 1), Box::from("bridge"))
        }
    };

    let name = routeros_name(radio);
    if let Some(name) = name
        && interfaces.iter().any(|i| i.name() == name)
    {
        return Err(Error::NameTaken {
            device: device.name().into(),
            name: name.into(),
        });
    }
    let in_bridge = |port: &InterfaceAccess| {
        port.bridge().map(|b| Target::Existing(b.id().0)) == Some(bridge)
    };
    let radio_role = radio.data().and_then(|d| d.radio.as_ref()).map(|r| r.role);
    for (port, new_name, new_role) in [
        (&ether, None, None),
        (radio, name, (radio_role != Some(role)).then_some(role)),
    ] {
        let to_bridge = !in_bridge(port);
        if to_bridge || new_name.is_some() || new_role.is_some() {
            plan.steps.push(PlanStep::UpdatePort(UpdatePort {
                interface: interface_ref(port),
                name: new_name.map(Box::from),
                rf_role: new_role,
                rf_role_before: radio_role.filter(|_| new_role.is_some()),
                bridge: to_bridge.then(|| bridge_name.clone()),
                bridge_id: to_bridge.then_some(bridge),
                bridge_before: port.bridge().map(|b| b.id().0),
            }));
        }
    }
    Ok(Side {
        device: device.clone(),
        radio: radio.clone(),
        router_port,
        bridge,
        bridge_name,
        address: address.clone(),
    })
}

/// The name the radio must get to be the one RouterOS has; none if it has it.
fn routeros_name(radio: &InterfaceAccess) -> Option<&'static str> {
    match (radio.is_sixty_ghz(), radio.external_port()) {
        (true, Some(PhysicalPortId::W60g(_))) => None,
        (true, _) => Some("wlan60-1"),
        (false, Some(PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_))) => None,
        (false, _) => Some("wlan1"),
    }
}

/// A radio port: NetBox type wireless, or a RouterOS radio name.
fn is_radio(interface: &InterfaceAccess) -> bool {
    interface.data().and_then(|d| d.port_type) == Some(PortType::Wireless)
        || matches!(
            interface.external_port(),
            Some(PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_) | PhysicalPortId::W60g(_))
        )
}

/// The wireless link between the two radios: created, or given what it lacks.
fn radios(plan: &mut NetboxPlan, sides: &[Side], ssid: Option<&str>) -> Result<(), Error> {
    let (ap, station) = (&sides[0], &sides[1]);
    for (side, other) in [(ap, station), (station, ap)] {
        if let Some(link) = side.radio.wireless_link()
            && link.peer != Some(other.radio.id())
        {
            return Err(Error::WirelessLinkElsewhere {
                port: port_name(&side.radio),
                far: side.radio.wireless_peer().as_ref().map(port_name),
            });
        }
    }
    let wanted = ssid.filter(|ssid| !ssid.is_empty());
    let mut changed = plan.steps.iter().any(|step| {
        matches!(step, PlanStep::UpdatePort(update)
            if (update.interface.id == ap.radio.id().0 || update.interface.id == station.radio.id().0)
                && (update.rf_role.is_some() || update.name.is_some()))
    });
    match ap.radio.wireless_link() {
        Some(end) => {
            let link = &end.link;
            let ssid = match wanted {
                Some(wanted) if wanted != &*link.ssid => Some(wanted.into()),
                None if link.ssid.is_empty() => Some(ap.device.name().into()),
                _ => None,
            };
            let psk = match (&link.psk, link.encrypted) {
                (Some(_), _) => None,
                (None, true) => return Err(Error::PskUnreadable(port_name(&ap.radio))),
                (None, false) => Some(new_psk()),
            };
            if ssid.is_some() || psk.is_some() {
                changed = true;
                plan.steps.push(PlanStep::UpdateWirelessLink(UpdateWirelessLink {
                    id: end.id,
                    a: interface_ref(&ap.radio),
                    b: interface_ref(&station.radio),
                    ssid,
                    ssid_before: link.ssid.clone(),
                    psk_set: psk.is_some(),
                    psk,
                }));
            }
        }
        None => {
            changed = true;
            plan.steps.push(PlanStep::CreateWirelessLink(CreateWirelessLink {
                a: interface_ref(&ap.radio),
                b: interface_ref(&station.radio),
                ssid: wanted.unwrap_or(ap.device.name()).into(),
                psk: new_psk(),
            }));
        }
    }
    // the station only hears the AP again once both have the new radio settings
    if changed {
        plan.apply_together = vec![device_ref(&ap.device), device_ref(&station.device)];
    }
    Ok(())
}

/// The segment's IPv4 network and an address in it with `ospf` for each router port and bridge.
async fn segment(
    topology: &Arc<Topology>,
    plan: &mut NetboxPlan,
    sides: &[Side],
    routes: &mut Option<Routes>,
) -> Result<(), Error> {
    // a router port in a bridge has its address on the bridge
    let l3 = |port: &InterfaceAccess| port.bridge().unwrap_or_else(|| port.clone());
    let mut ends = Vec::new();
    for side in sides {
        let router = l3(&side.router_port);
        ends.push(End {
            device: router.device().expect("a router port"),
            on: Target::Existing(router.id().0),
            name: router.name().into(),
            interface: Some(router),
            wanted: None,
        });
    }
    for side in sides {
        let existing = match side.bridge {
            Target::Existing(id) => topology.get_interface_by_id(crate::topology::InterfaceId(id)),
            Target::Step(_) => None,
        };
        ends.push(End {
            device: side.device.clone(),
            interface: existing,
            on: side.bridge,
            name: side.bridge_name.clone(),
            wanted: side.address.clone(),
        });
    }
    let has = |end: &End| end.interface.as_ref().and_then(ipv4);
    // the network a router port has, the AP's first
    let mut net = None;
    for end in &ends[..2] {
        if let Some(address) = has(end) {
            match net {
                None => net = Some(address.trunc()),
                Some(net) if net == address.trunc() => {}
                Some(_) => {
                    return Err(Error::PortAddressed {
                        port: end_name(end),
                        address: address.to_string().into(),
                    });
                }
            }
        }
    }
    let ends_on = ends.len() as u8;
    let (net, prefix) = match net {
        Some(net) => (net, net.to_string()),
        None => {
            let routes = read(topology, plan, routes).await?;
            let (pool, net) = next_free_network(topology, routes, plan).await?;
            plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
                prefix: net.to_string().into(),
                pool: pool.to_string().into(),
                description: format!(
                    "{} – {}",
                    ends[0].device.name(),
                    ends[1].device.name()
                )
                .into(),
                site: None,
                vlan: None,
            }));
            (net, net.to_string())
        }
    };
    let IpNet::V4(v4) = net else {
        unreachable!("an IPv4 address")
    };
    if v4.hosts().count() < ends.len() {
        return Err(Error::NetworkTooSmall {
            net: net.to_string().into(),
            ends: ends_on,
        });
    }
    for end in &ends {
        match has(end) {
            Some(address) if address.trunc() == net => {}
            Some(address) => {
                return Err(Error::PortAddressed {
                    port: end_name(end),
                    address: address.to_string().into(),
                });
            }
            None => {
                let address = match &end.wanted {
                    Some(wanted) => {
                        let address = wanted
                            .parse::<IpNet>()
                            .ok()
                            .filter(|address| address.trunc() == net)
                            .ok_or_else(|| Error::PortAddressed {
                                port: end_name(end),
                                address: wanted.as_str().into(),
                            })?;
                        if taken(topology, plan, net).contains(&address.addr()) {
                            return Err(Error::NetboxChanged(
                                format!("{address} is taken").into(),
                            ));
                        }
                        address
                    }
                    None => {
                        let routes = read(topology, plan, routes).await?;
                        next_host(topology, plan, routes, v4)?
                    }
                };
                plan.steps.push(PlanStep::CreateAddress(CreateAddress {
                    address: address.to_string().into(),
                    device: device_ref(&end.device),
                    interface: end.name.clone(),
                    prefix: prefix.clone().into(),
                    on: end.on,
                }));
            }
        }
        if let Some(interface) = &end.interface
            && !interface.use_ospf()
        {
            plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
                interface: interface_ref(interface),
                label: None,
                label_before: interface.label().map(Box::from),
                mtu: None,
                mtu_before: interface.mtu(),
                tags_added: vec![InterfaceTag::Ospf],
                tags_removed: Vec::new(),
                poe: None,
                poe_before: None,
            }));
        }
    }
    Ok(())
}

fn end_name(end: &End) -> Box<str> {
    format!("{} {}", end.device.name(), end.name).into()
}

fn ipv4(interface: &InterfaceAccess) -> Option<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .find(|net| matches!(net, IpNet::V4(_)))
}

async fn read<'r>(
    topology: &Arc<Topology>,
    plan: &mut NetboxPlan,
    routes: &'r mut Option<Routes>,
) -> Result<&'r Routes, Error> {
    if routes.is_none() {
        *routes = Some(Routes::read(topology).await?);
    }
    let routes = routes.as_ref().expect("read above");
    plan.checked_with = Some(routes.router.clone());
    Ok(routes)
}

/// The hosts of `net` NetBox or the plan hands out already.
fn taken(topology: &Topology, plan: &NetboxPlan, net: IpNet) -> Vec<IpAddr> {
    let mut taken = topology.addresses_in(net);
    taken.extend(plan.steps.iter().filter_map(|step| match step {
        PlanStep::CreateAddress(create) => create
            .address
            .parse::<IpNet>()
            .ok()
            .map(|address| address.addr())
            .filter(|host| net.contains(host)),
        _ => None,
    }));
    taken
}

/// The first host of `net` neither NetBox nor the plan has and the network doesn't route.
fn next_host(
    topology: &Topology,
    plan: &mut NetboxPlan,
    routes: &Routes,
    net: Ipv4Net,
) -> Result<IpNet, Error> {
    let taken = taken(topology, plan, IpNet::V4(net));
    for host in net.hosts() {
        let host = IpAddr::V4(host);
        if taken.contains(&host) {
            continue;
        }
        match routes.in_use(host, IpNet::V4(net)) {
            Some(routed) => {
                if !plan.undocumented.iter().any(|u| u.route == routed.route) {
                    plan.undocumented.push(routed);
                }
            }
            None => {
                return Ok(IpNet::new(host, net.prefix_len()).expect("a host of the network"));
            }
        }
    }
    Err(Error::NetworkTooSmall {
        net: net.to_string().into(),
        ends: 4,
    })
}

/// The length of a new segment network: room for two routers and two bridges.
const SEGMENT_LENGTH: u8 = 29;
/// How many free networks of the pool are looked at, as `connection::CANDIDATES`.
const CANDIDATES: usize = 32;

/// The first `/29` of the transfer pool NetBox has free, without addresses in it, that the
/// network doesn't route, and the pool prefix it is in.
async fn next_free_network(
    topology: &Arc<Topology>,
    routes: &Routes,
    plan: &mut NetboxPlan,
) -> Result<(IpNet, IpNet), Error> {
    let rest = NetboxRest::reader()?;
    let mut looked_at = 0;
    for (id, pool) in topology.pool_prefixes(PrefixPool::Transfer, false) {
        let free = rest
            .get(&format!("ipam/prefixes/{}/available-prefixes/", id.0))
            .await?;
        let mut candidates = Vec::new();
        for block in free.as_array().into_iter().flatten() {
            if let Some(Ok(block)) = block["prefix"].as_str().map(str::parse::<IpNet>)
                && let Ok(subnets) = block.subnets(SEGMENT_LENGTH)
            {
                candidates.extend(subnets.take(CANDIDATES));
            }
        }
        for net in candidates {
            looked_at += 1;
            if looked_at > CANDIDATES {
                return Err(Error::NoFreeAddress("transfer", 4));
            }
            if let Some(routed) = routes.overlapping(net, pool) {
                if !plan.undocumented.iter().any(|u| u.route == routed.route) {
                    plan.undocumented.push(routed);
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

/// A device with a radio, for the form: its radio and what the link and the cable have.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RadioBridgeCandidate {
    pub device: DeviceRef,
    pub radio: InterfaceRef,
    pub role: Option<RadioRole>,
    /// the device at the other end of the radio's wireless link
    pub peer: Option<DeviceRef>,
    /// the router port an Ethernet port of it is cabled to
    pub router_port: Option<InterfaceRef>,
    /// the IPv4 address of its `bridge`
    pub address: Option<Box<str>>,
}

/// The devices with RouterOS and a single radio, by name: the bridges to choose from.
pub fn candidates(topology: &Arc<Topology>) -> Vec<RadioBridgeCandidate> {
    let mut found = topology
        .list_devices()
        .filter(DeviceAccess::has_routeros)
        .filter_map(|device| {
            let interfaces = device.interfaces();
            let radios = interfaces.iter().filter(|i| is_radio(i)).collect::<Vec<_>>();
            let [radio] = radios[..] else { return None };
            let router_port = interfaces
                .iter()
                .filter(|i| i.is_ethernet_port())
                .flat_map(|port| port.connected_interfaces())
                .find(|far| far.device().is_some_and(|d| d.has_routeros()));
            let address = interfaces
                .iter()
                .filter(|i| i.data().and_then(|d| d.port_type) == Some(PortType::Bridge))
                .find_map(ipv4)
                .map(|net| net.to_string().into());
            Some(RadioBridgeCandidate {
                device: device_ref(&device),
                radio: interface_ref(radio),
                role: radio.data().and_then(|d| d.radio.as_ref()).map(|r| r.role),
                peer: radio
                    .wireless_peer()
                    .and_then(|peer| peer.device())
                    .as_ref()
                    .map(device_ref),
                router_port: router_port.as_ref().map(interface_ref),
                address,
            })
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.device.name.cmp(&b.device.name));
    found
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::device::ros::netbox_example_test::example_topology;

    fn find(topology: &Arc<Topology>, name: &str) -> DeviceAccess {
        topology
            .list_devices()
            .find(|d| d.name() == name)
            .unwrap_or_else(|| panic!("{name}"))
    }

    fn input(ap: &DeviceAccess, station: &DeviceAccess) -> RadioBridgeInput {
        RadioBridgeInput {
            ap: ap.id().0,
            station: station.id().0,
            ap_router_port: None,
            station_router_port: None,
            ap_address: None,
            station_address: None,
            ssid: None,
        }
    }

    /// schopf-burri – burri-schopf as in the export: cables, bridges, radios, link and
    /// loopbacks are there, the bridges lack their addresses in router-schopf's network and
    /// `ospf`, berghof-burri an IPv4 in it.
    #[tokio::test]
    async fn schopf_burri_gets_its_addresses() {
        let topology = example_topology();
        let (ap, station) = (find(&topology, "schopf-burri"), find(&topology, "burri-schopf"));
        let mut routes = Some(Routes::none("test"));
        let plan = plan_with(&topology, &input(&ap, &station), &mut routes)
            .await
            .expect("plan");
        let addresses = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::CreateAddress(create) => {
                    Some((create.device.name.to_string(), create.address.to_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(addresses.len(), 3, "{:#?}", plan.steps);
        for (device, address) in &addresses {
            let address = address.parse::<IpNet>().expect("an address");
            assert_eq!(address.trunc().to_string(), "172.17.0.40/29", "{device}");
            assert_ne!(address.addr().to_string(), "172.17.0.41", "{device}");
        }
        let devices = addresses.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>();
        assert_eq!(devices, ["berghof-burri", "schopf-burri", "burri-schopf"]);
        let tagged = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::UpdateInterface(update) if update.tags_added == [InterfaceTag::Ospf] => {
                    Some(update.interface.device.name.to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tagged, ["schopf-burri", "burri-schopf"], "the routers have ospf");
        assert!(
            !plan.steps.iter().any(|step| matches!(
                step,
                PlanStep::CreateCable(_) | PlanStep::CreateInterface(_) | PlanStep::CreateWirelessLink(_)
            )),
            "{:#?}",
            plan.steps
        );
    }

    /// A 60 GHz radio named `wlan1` becomes `wlan60-1`, another radio `wlan1`.
    #[test]
    fn radios_get_their_routeros_name() {
        use crate::topology::{Device, Interface, builder::TopologyBuilder};
        use std::str::FromStr;
        let mut builder = TopologyBuilder::default();
        let device = builder.devices.next_id();
        builder.devices.insert(device, Device::default());
        let mut ids = Vec::new();
        for (name, sixty_ghz) in [("wlan1", true), ("wlan60-1", true), ("radio", false), ("wlan1", false)] {
            let id = builder.interfaces.next_id();
            builder.interfaces.insert(
                id,
                Interface {
                    name: name.into(),
                    device,
                    external: PhysicalPortId::from_str(name).ok(),
                    port_type: Some(PortType::Wireless),
                    sixty_ghz,
                    ..Interface::default()
                },
            );
            ids.push(id);
        }
        let topology = Arc::new(builder.build());
        let names = ids
            .into_iter()
            .map(|id| routeros_name(&topology.get_interface_by_id(id).unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(names, [Some("wlan60-1"), None, Some("wlan1"), None]);
    }

    /// Radios without RF role get theirs, the link without SSID and PSK both, and the station
    /// is cut off until both ends are applied.
    #[test]
    fn a_radio_without_role_is_corrected() {
        let topology = example_topology();
        let (ap, station) = (find(&topology, "hegglin-schopf"), find(&topology, "schopf-hegglin"));
        // without loopbacks and segment: the export's devices lack `lo`, a new one asks NetBox
        let (plan, _) = link_plan(&topology, &input(&ap, &station)).expect("plan");
        let ports = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::UpdatePort(update) => Some(update),
                _ => None,
            })
            .filter(|update| update.rf_role.is_some())
            .collect::<Vec<_>>();
        assert_eq!(ports.len(), 2, "{:#?}", plan.steps);
        assert!(ports.iter().all(|update| update.name.is_none()), "named as RouterOS");
        let link = plan.steps.iter().find_map(|step| match step {
            PlanStep::UpdateWirelessLink(update) => Some(update),
            _ => None,
        });
        let link = link.expect("the link gets SSID and PSK");
        assert_eq!(link.ssid.as_deref(), Some("hegglin-schopf"));
        assert!(link.psk_set);
        assert_eq!(
            plan.apply_together.iter().map(|d| d.name.to_string()).collect::<Vec<_>>(),
            ["hegglin-schopf", "schopf-hegglin"]
        );
        assert!(plan.cut_off.is_empty());
    }

    /// The same device on both ends is refused.
    #[tokio::test]
    async fn one_device_is_no_link() {
        let topology = example_topology();
        let ap = find(&topology, "schopf-burri");
        let mut routes = Some(Routes::none("test"));
        assert!(matches!(
            plan_with(&topology, &input(&ap, &ap), &mut routes).await,
            Err(Error::SameDevice(_))
        ));
    }

    /// The bridges of the export are candidates, with the device at the other end of the link.
    #[test]
    fn bridges_are_candidates_with_their_peer() {
        let topology = example_topology();
        let candidates = candidates(&topology);
        let schopf = candidates
            .iter()
            .find(|c| &*c.device.name == "schopf-burri")
            .expect("schopf-burri");
        assert_eq!(schopf.peer.as_ref().map(|p| &*p.name), Some("burri-schopf"));
        assert_eq!(
            schopf.router_port.as_ref().map(|p| &*p.device.name),
            Some("router-schopf")
        );
    }
}
