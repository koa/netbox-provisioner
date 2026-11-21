//! Scope: The helper „Funkbrücke“ for bridge devices without RouterOS; not the one with RouterOS
//! (`radio_bridge`).
//!
//! Helper „Funkbrücke“ for two bridge devices without RouterOS (two UISP Wave Nano,
//! `docs/foreign-radio-bridge.md`). They are set up by hand as an L2 bridge
//! with a DHCP client, so the plan is the NetBox side and the routers':
//!
//! - the cable from each router port to the bridge's Ethernet port, where it lacks, and as label
//!   at each cable end the device at the other end (as `radio_bridge`);
//! - the radios' RF roles and the wireless link between them, with an SSID; a new link gets a
//!   PSK, the one to set on the radios, an existing one none: its radios run with a key of
//!   their own;
//! - a `/31` of the transfer pool between the two router ports, for OSPFv2, which forms
//!   neighbors within a subnet only (OSPFv3 runs over the link-local addresses);
//! - on each router port a network of its own, a `/29` of the transfer pool with its first
//!   address on the port, `dhcp` and a short lease (`LEASE_SECONDS`): a bridge takes the first
//!   offer and, that router or the link gone, one of the router left within a lease, so each
//!   bridge stays reachable through a router.
//!
//! Each router port gets `ospf` and `dhcp`. A router port in a bridge is refused
//! (`PortInBridge`): its networks would go on the bridge and all its ports.
//! What is there stays: the network both router ports have is the transfer network, the one
//! only a port has its DHCP network; another one stops the helper with an error naming it.
use super::{
    CreateAddress, CreatePrefix, NetboxPlan, PlanStep, Target, UpdateInterface, UpdatePort,
    device_ref, flags::InterfaceTag, port_name, radio_bridge, radio_bridge::RadioBridgeInput,
    routes::Routes,
};
use crate::helper::FreeNetwork;
use crate::{
    Error,
    topology::{
        DeviceId, RadioRole, Topology,
        access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess},
        findings::interface_ref,
        natural::naturalize_interface,
    },
};
use ipnet::IpNet;
use std::sync::Arc;

/// The lease of a bridge's DHCP network: it moves to the other router within this once its own
/// is gone (the concept: 1–2 min).
pub const LEASE_SECONDS: u32 = 120;
/// A DHCP network: the router and room for the bridges.
const DHCP_LENGTH: u8 = 29;

/// One bridge device of the link and what the plan found of it.
struct Side {
    device: DeviceAccess,
    /// the Ethernet port cabled to the router
    ether: InterfaceAccess,
    router_port: InterfaceAccess,
}

/// The plan for two bridge devices without RouterOS, with `routes` read once an address is
/// needed (tests give them).
pub(super) async fn plan_with(
    topology: &Arc<Topology>,
    input: &RadioBridgeInput,
    routes: &mut Option<Routes>,
) -> Result<NetboxPlan, Error> {
    let device = |id| {
        topology
            .get_device_by_id(&DeviceId(id))
            .ok_or(Error::UnknownDevice(id))
    };
    let (ap, station) = (device(input.ap)?, device(input.station)?);
    if ap.id() == station.id() {
        return Err(Error::SameDevice(ap.name().into()));
    }
    // a bridge with RouterOS on one side only: the AP has none
    if station.has_routeros() {
        return Err(Error::NoRouterOs(ap.name().into()));
    }
    let mut plan = NetboxPlan::new(
        &ap,
        format!("Funkbrücke {} – {}", ap.name(), station.name()),
    );
    let mut sides = Vec::new();
    for (device, router_port, other) in [
        (&ap, input.ap_router_port, &station),
        (&station, input.station_router_port, &ap),
    ] {
        let radio_bridge::RouterCable { ether, router_port } =
            radio_bridge::router_cable(topology, &mut plan, device, other, router_port)?;
        // its networks, `dhcp` and `ospf` would go on the bridge and all its ports (1og-sw2
        // ether13 in its client bridge): the port leaves it first („Aus Bridge lösen“)
        if let Some(bridge) = router_port.bridge() {
            return Err(Error::PortInBridge {
                port: port_name(&router_port),
                bridge: bridge.name().into(),
            });
        }
        sides.push(Side {
            device: device.clone(),
            ether,
            router_port,
        });
    }
    let pairs = pairs(&ap, &station)?;
    for pair in &pairs {
        for (radio, role) in [
            (&pair.ap, RadioRole::Ap),
            (&pair.station, RadioRole::Station),
        ] {
            let radio_role = radio.data().and_then(|d| d.radio.as_ref()).map(|r| r.role);
            if radio_role != Some(role) {
                plan.steps.push(PlanStep::UpdatePort(UpdatePort {
                    interface: interface_ref(radio),
                    name: None,
                    rf_role: Some(role),
                    rf_role_before: radio_role,
                    bridge: None,
                    bridge_id: None,
                    bridge_before: radio.bridge().map(|b| b.id().0),
                    leaves_bridge: None,
                }));
            }
        }
    }
    let routers = sides
        .iter()
        .map(|side| side.router_port.device().expect("a router port"))
        .collect::<Vec<_>>();
    if routers[0].id() == routers[1].id() {
        return Err(Error::SameDevice(routers[0].name().into()));
    }
    // the preview generates for the AP's router: the bridge has no config to generate
    plan.device = routers[0].clone();
    // the radios are set by hand: nothing to apply together
    for RadioPair {
        ap: ap_radio,
        station: station_radio,
    } in &pairs
    {
        radio_bridge::radios(
            &mut plan,
            radio_bridge::RadioLink {
                ap: ap_radio,
                ap_device: &ap,
                station: station_radio,
            },
            input.ssid.as_deref(),
            false,
        )?;
    }
    networks(topology, &mut plan, &sides, routes).await?;
    let cables = sides
        .iter()
        .map(|side| radio_bridge::BridgeCable {
            router_port: side.router_port.clone(),
            ether: side.ether.clone(),
            device: side.device.clone(),
        })
        .collect::<Vec<_>>();
    radio_bridge::labels(&mut plan, &cables);
    Ok(plan)
}

/// A radio of the AP and the station's that make a link ([`pairs`]).
struct RadioPair {
    ap: InterfaceAccess,
    station: InterfaceAccess,
}

/// The radios of the AP and the station that make a link: by the wireless link one has, else
/// by name (a Wave Nano's 60 GHz `wlan0` and its 5 GHz backup `wlan1`). Each radio has one.
fn pairs(ap: &DeviceAccess, station: &DeviceAccess) -> Result<Vec<RadioPair>, Error> {
    // in NetBox' order, so the plan's steps are too
    let radios = |device: &DeviceAccess| {
        let mut radios = device
            .interfaces()
            .into_iter()
            .filter(radio_bridge::is_radio)
            .collect::<Vec<_>>();
        radios.sort_by_cached_key(|radio| naturalize_interface(radio.name()));
        radios
    };
    let (ap_radios, station_radios) = (radios(ap), radios(station));
    for (device, radios) in [(ap, &ap_radios), (station, &station_radios)] {
        if radios.is_empty() {
            return Err(Error::NoSingleRadio(device.name().into()));
        }
    }
    let mut pairs = Vec::new();
    for radio in &ap_radios {
        let peer = match radio.wireless_link() {
            Some(link) => station_radios
                .iter()
                .find(|peer| link.peer == Some(peer.id()))
                .ok_or_else(|| Error::WirelessLinkElsewhere {
                    port: port_name(radio),
                    far: radio.wireless_peer().as_ref().map(port_name),
                })?,
            None => station_radios
                .iter()
                .find(|peer| peer.name() == radio.name() && peer.wireless_link().is_none())
                .ok_or_else(|| Error::RadioUnpaired(port_name(radio)))?,
        };
        pairs.push(RadioPair {
            ap: radio.clone(),
            station: peer.clone(),
        });
    }
    if let Some(unpaired) = station_radios
        .iter()
        .find(|radio| !pairs.iter().any(|pair| pair.station.id() == radio.id()))
    {
        return Err(Error::RadioUnpaired(port_name(unpaired)));
    }
    Ok(pairs)
}

/// The transfer network between the router ports and each one's DHCP network, with an address
/// of each on the port (or its bridge) and the tags `ospf` and `dhcp` there.
async fn networks(
    topology: &Arc<Topology>,
    plan: &mut NetboxPlan,
    sides: &[Side],
    routes: &mut Option<Routes>,
) -> Result<(), Error> {
    let ends = [sides[0].router_port.clone(), sides[1].router_port.clone()];
    let nets = ends.each_ref().map(ipv4s);
    let shared = nets[0]
        .iter()
        .map(IpNet::trunc)
        .find(|net| nets[1].iter().any(|other| other.trunc() == *net));
    if shared.is_none() {
        let routes = radio_bridge::read(topology, plan, routes).await?;
        let FreeNetwork { pool, net } =
            radio_bridge::next_free_network(topology, routes, plan, 31).await?;
        plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
            prefix: net.to_string().into(),
            pool: pool.to_string().into(),
            description: format!(
                "{} – {}",
                sides[0].router_port.device().expect("a router port").name(),
                sides[1].router_port.device().expect("a router port").name()
            )
            .into(),
            site: None,
            vlan: None,
            dhcp_lease_time: None,
        }));
        for (end, address) in [(&ends[0], net.network()), (&ends[1], net.broadcast())] {
            plan.steps.push(PlanStep::CreateAddress(CreateAddress {
                address: IpNet::new(address, 31).expect("a /31").to_string().into(),
                device: device_ref(&end.device().expect("a router port")),
                interface: end.name().into(),
                prefix: net.to_string().into(),
                on: Target::Existing(end.id().0),
            }));
        }
    }
    for ((end, nets), side) in ends.iter().zip(&nets).zip(sides) {
        let router = end.device().expect("a router port");
        let own = nets
            .iter()
            .filter(|net| Some(net.trunc()) != shared)
            .collect::<Vec<_>>();
        match own[..] {
            [] => {
                let routes = radio_bridge::read(topology, plan, routes).await?;
                let FreeNetwork { pool, net } =
                    radio_bridge::next_free_network(topology, routes, plan, DHCP_LENGTH).await?;
                plan.steps.push(PlanStep::CreatePrefix(CreatePrefix {
                    prefix: net.to_string().into(),
                    pool: pool.to_string().into(),
                    description: format!("{} – {} (DHCP)", router.name(), side.device.name())
                        .into(),
                    site: None,
                    vlan: None,
                    dhcp_lease_time: Some(LEASE_SECONDS),
                }));
                let IpNet::V4(v4) = net else {
                    unreachable!("the transfer pool is IPv4")
                };
                let address = radio_bridge::next_host(topology, plan, routes, v4)?;
                plan.steps.push(PlanStep::CreateAddress(CreateAddress {
                    address: address.to_string().into(),
                    device: device_ref(&router),
                    interface: end.name().into(),
                    prefix: net.to_string().into(),
                    on: Target::Existing(end.id().0),
                }));
            }
            // a /31 or /32 leaves no address for a bridge's lease
            [net] if net.prefix_len() < 31 => {}
            [net, ..] => {
                return Err(Error::PortAddressed {
                    port: port_name(end),
                    address: net.to_string().into(),
                });
            }
        }
        let tags_added = [
            (InterfaceTag::Ospf, end.use_ospf()),
            (InterfaceTag::Dhcp, end.is_enable_dhcp_server()),
        ]
        .into_iter()
        .filter(|(_, has)| !has)
        .map(|(tag, _)| tag)
        .collect::<Vec<_>>();
        if !tags_added.is_empty() {
            plan.steps.push(PlanStep::UpdateInterface(UpdateInterface {
                interface: interface_ref(end),
                label: None,
                label_before: end.label().map(Box::from),
                mtu: None,
                mtu_before: end.mtu(),
                tags_added: tags_added.into(),
                tags_removed: Vec::new(),
                poe: None,
                poe_before: None,
                poe_removed: false,
            }));
        }
    }
    Ok(())
}

fn ipv4s(interface: &InterfaceAccess) -> Vec<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .filter(|net| matches!(net, IpNet::V4(_)))
        .collect()
}

#[cfg(test)]
mod test;
