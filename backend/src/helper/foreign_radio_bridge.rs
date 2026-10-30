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
//! Each router port gets `ospf` and `dhcp`, its addresses go on its bridge where it is in one.
//! What is there stays: the network both router ports have is the transfer network, the one
//! only a port has its DHCP network; another one stops the helper with an error naming it.
use super::{
    CreateAddress, CreatePrefix, NetboxPlan, PlanStep, Target, UpdateInterface, UpdatePort,
    device_ref, flags::InterfaceTag, port_name, radio_bridge, radio_bridge::RadioBridgeInput,
    routes::Routes,
};
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
        let (ether, router_port) =
            radio_bridge::router_cable(topology, &mut plan, device, other, router_port)?;
        sides.push(Side {
            device: device.clone(),
            ether,
            router_port,
        });
    }
    let pairs = pairs(&ap, &station)?;
    for (ap_radio, station_radio) in &pairs {
        for (radio, role) in [(ap_radio, RadioRole::Ap), (station_radio, RadioRole::Station)] {
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
    for (ap_radio, station_radio) in &pairs {
        radio_bridge::radios(
            &mut plan,
            [(ap_radio, &ap), (station_radio, &station)],
            input.ssid.as_deref(),
            false,
        )?;
    }
    networks(topology, &mut plan, &sides, routes).await?;
    let cables = sides
        .iter()
        .map(|side| (side.router_port.clone(), side.ether.clone(), side.device.clone()))
        .collect::<Vec<_>>();
    radio_bridge::labels(&mut plan, &cables);
    Ok(plan)
}

/// The radios of the AP and the station that make a link: by the wireless link one has, else
/// by name (a Wave Nano's 60 GHz `wlan0` and its 5 GHz backup `wlan1`). Each radio has one.
fn pairs(
    ap: &DeviceAccess,
    station: &DeviceAccess,
) -> Result<Vec<(InterfaceAccess, InterfaceAccess)>, Error> {
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
        pairs.push((radio.clone(), peer.clone()));
    }
    if let Some(unpaired) = station_radios
        .iter()
        .find(|radio| !pairs.iter().any(|(_, peer)| peer.id() == radio.id()))
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
    // a router port in a bridge has its addresses on the bridge
    let l3 = |port: &InterfaceAccess| port.bridge().unwrap_or_else(|| port.clone());
    let ends = [l3(&sides[0].router_port), l3(&sides[1].router_port)];
    let nets = ends.each_ref().map(ipv4s);
    let shared = nets[0]
        .iter()
        .map(IpNet::trunc)
        .find(|net| nets[1].iter().any(|other| other.trunc() == *net));
    if shared.is_none() {
        let routes = radio_bridge::read(topology, plan, routes).await?;
        let (pool, net) = radio_bridge::next_free_network(topology, routes, plan, 31).await?;
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
                let (pool, net) =
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
                tags_added,
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
mod test {
    use super::*;
    use crate::topology::{
        Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, PortType, RadioLink,
        WirelessLinkEnd, builder::TopologyBuilder,
    };

    /// The lab's link: `np1 ether8` – `wave-a` ~ `wave-b` – `np2 ether8`, each router port with
    /// the transfer network and a DHCP network of its own, `np2`'s without its tags.
    struct Lab {
        builder: TopologyBuilder,
        np1_port: InterfaceId,
        np2_port: InterfaceId,
        wave_a: DeviceId,
        wave_b: DeviceId,
    }

    impl Lab {
        fn new() -> Lab {
            let mut builder = TopologyBuilder::default();
            let np1 = device(&mut builder, "np1", true);
            let np2 = device(&mut builder, "np2", true);
            let np1_port = interface(&mut builder, np1, "ether8", PortType::Ethernet);
            let np2_port = interface(&mut builder, np2, "ether8", PortType::Ethernet);
            address(&mut builder, np1_port, "192.168.101.40/31");
            address(&mut builder, np1_port, "192.168.101.49/29");
            address(&mut builder, np2_port, "192.168.101.41/31");
            address(&mut builder, np2_port, "192.168.101.57/29");
            let port = builder.interfaces.get_mut(&np1_port).unwrap();
            port.use_ospf = true;
            port.enable_dhcp_server = true;
            let wave_a = device(&mut builder, "wave-a", false);
            let wave_b = device(&mut builder, "wave-b", false);
            let mut radios = Vec::new();
            for (wave, router_port) in [(wave_a, np1_port), (wave_b, np2_port)] {
                let eth0 = interface(&mut builder, wave, "eth0", PortType::Ethernet);
                let wlan0 = interface(&mut builder, wave, "wlan0", PortType::Wireless);
                builder.interfaces.get_mut(&wlan0).unwrap().sixty_ghz = true;
                let id = builder.cables.next_id();
                builder.cables.insert(
                    id,
                    Cable {
                        port_a: Box::new([CablePort::Interface(router_port)]),
                        port_b: Box::new([CablePort::Interface(eth0)]),
                        kind: None,
                    },
                );
                radios.push(wlan0);
            }
            for (radio, peer) in [(radios[0], radios[1]), (radios[1], radios[0])] {
                builder.interfaces.get_mut(&radio).unwrap().wireless_link = Some(WirelessLinkEnd {
                    id: 1,
                    peer: Some(peer),
                    link: RadioLink {
                        ssid: "wave-a".into(),
                        encrypted: false,
                        psk: None,
                        country: None,
                        frequency_mode: None,
                    },
                });
            }
            Lab {
                builder,
                np1_port,
                np2_port,
                wave_a,
                wave_b,
            }
        }

        async fn plan(self) -> Result<NetboxPlan, Error> {
            let input = RadioBridgeInput {
                ap: self.wave_a.0,
                station: self.wave_b.0,
                ap_router_port: None,
                station_router_port: None,
                ap_address: None,
                station_address: None,
                ssid: None,
            };
            let topology = Arc::new(self.builder.build());
            let mut routes = Some(Routes::none("test"));
            radio_bridge::plan_with(&topology, &input, &mut routes).await
        }
    }

    fn device(builder: &mut TopologyBuilder, name: &str, has_routeros: bool) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros,
                ..Device::default()
            },
        );
        id
    }

    fn interface(
        builder: &mut TopologyBuilder,
        device: DeviceId,
        name: &str,
        port_type: PortType,
    ) -> InterfaceId {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                external: name.parse().ok(),
                port_type: Some(port_type),
                ..Interface::default()
            },
        );
        id
    }

    fn address(builder: &mut TopologyBuilder, interface: InterfaceId, ip: &str) {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: ip.parse().unwrap(),
                interface: None,
                prefix: None,
            },
        );
        let interface = builder.interfaces.get_mut(&interface).unwrap();
        interface.ips = interface.ips.iter().copied().chain([id]).collect();
    }

    /// The link as the lab has it: no network, address, cable or wireless link is made; the
    /// radios get their roles, `np2`'s port its tags, the cable ends their labels.
    #[tokio::test]
    async fn the_lab_link_gets_only_what_it_lacks() {
        let plan = Lab::new().plan().await.expect("plan");
        assert!(
            !plan.steps.iter().any(|step| matches!(
                step,
                PlanStep::CreatePrefix(_)
                    | PlanStep::CreateAddress(_)
                    | PlanStep::CreateCable(_)
                    | PlanStep::CreateWirelessLink(_)
                    | PlanStep::UpdateWirelessLink(_)
            )),
            "an existing link without a PSK keeps it so: {:#?}",
            plan.steps
        );
        let roles = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::UpdatePort(update) => {
                    Some((update.interface.device.name.to_string(), update.rf_role))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            [
                ("wave-a".into(), Some(RadioRole::Ap)),
                ("wave-b".into(), Some(RadioRole::Station))
            ]
        );
        let tags = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::UpdateInterface(update) if !update.tags_added.is_empty() => Some((
                    update.interface.device.name.to_string(),
                    update.tags_added.clone(),
                    update.label.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        // the label goes into the same update
        assert_eq!(
            tags,
            [(
                "np2".into(),
                vec![InterfaceTag::Ospf, InterfaceTag::Dhcp],
                Some("wave-b".into())
            )]
        );
        assert!(plan.apply_together.is_empty(), "the radios are set by hand");
        assert_eq!(plan.device().name(), "np1", "the preview's device");
    }

    /// A second network only one router port has can't be told from its DHCP network.
    #[tokio::test]
    async fn another_network_at_a_router_port_stops_it() {
        let mut lab = Lab::new();
        let port = lab.np2_port;
        address(&mut lab.builder, port, "10.9.9.1/30");
        let Err(error) = lab.plan().await else { panic!("two networks of its own") };
        assert!(
            matches!(&error, Error::PortAddressed { port, .. } if &**port == "np2 ether8"),
            "{error:?}"
        );
    }

    /// A bridge without RouterOS facing one with RouterOS is no link either kind sets up.
    #[tokio::test]
    async fn a_routeros_station_is_refused() {
        let mut lab = Lab::new();
        let station = lab.wave_b;
        lab.builder.devices.get_mut(&station).unwrap().has_routeros = true;
        let Err(error) = lab.plan().await else { panic!("mixed") };
        assert!(matches!(&error, Error::NoRouterOs(name) if &**name == "wave-a"), "{error:?}");
    }

    /// A router port to a Wave Nano is a radio link in the interface view, also with `dhcp`
    /// (the Wave Nano's lease), and its path names the bridge device's port.
    #[test]
    fn a_router_port_to_a_wave_nano_is_a_radio_link() {
        use crate::topology::interfaces::InterfaceTask;
        let topology = Arc::new(Lab::new().builder.build());
        let np1 = topology
            .list_devices()
            .find(|d| d.name() == "np1")
            .expect("np1");
        let view = np1.interface_view();
        let row = view
            .rows
            .iter()
            .find(|row| &*row.interface.name == "ether8")
            .expect("ether8");
        assert_eq!(row.task, InterfaceTask::RadioLink);
        let path = row.radio_path.as_ref().expect("a radio path");
        assert_eq!(
            path.bridge_port.as_ref().map(|p| &*p.device.name),
            Some("wave-a")
        );
    }

    /// schmidrueti-berg – berg-schmidrueti as in the export: Wave Nanos with the 60 GHz
    /// `wlan0` and the 5 GHz backup `wlan1`, each linked to the other's, without SSID. Both are
    /// candidates by their `wlan0`, the radios pair by their links, and switch-schmidrueti's
    /// port to one is a radio link the helper is suggested for.
    #[test]
    fn the_wave_nanos_of_the_export_pair_their_radios() {
        use crate::topology::{
            interfaces::{ActionHelper, ActionKind, InterfaceTask},
            test::example_topology,
        };
        let topology = example_topology();
        let find = |name: &str| {
            topology
                .list_devices()
                .find(|d| d.name() == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        let (ap, station) = (find("schmidrueti-berg"), find("berg-schmidrueti"));
        let candidates = radio_bridge::candidates(&topology);
        let candidate = candidates
            .iter()
            .find(|c| c.device.id == ap.id().0)
            .expect("a candidate");
        assert!(!candidate.routeros);
        assert_eq!(&*candidate.radio.name, "wlan0");
        assert_eq!(
            candidate.peer.as_ref().map(|p| &*p.name),
            Some("berg-schmidrueti")
        );
        // over patch panels: switch-schmidrueti at one end, 1og-sw2 at the other
        let mut plan = NetboxPlan::new(&ap, "test");
        for (device, other, router_port) in [
            (&ap, &station, "switch-schmidrueti ether1"),
            (&station, &ap, "1og-sw2 ether13"),
        ] {
            let (_, port) = radio_bridge::router_cable(&topology, &mut plan, device, other, None)
                .expect("a router port");
            assert_eq!(&*port_name(&port), router_port);
        }
        let pairs = pairs(&ap, &station)
            .expect("pairs")
            .iter()
            .map(|(a, b)| (a.name().to_string(), b.name().to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            pairs,
            [("wlan0".into(), "wlan0".into()), ("wlan1".into(), "wlan1".into())]
        );
        // over two patch panels
        let router = find("switch-schmidrueti");
        let view = router.interface_view();
        let row = view
            .rows
            .iter()
            .find(|row| &*row.interface.name == "ether1")
            .expect("ether1");
        assert_eq!(row.task, InterfaceTask::RadioLink);
        assert!(
            row.actions.iter().any(|action| action.kind == ActionKind::Suggestion
                && action.helper == ActionHelper::RadioBridge),
            "{:?}",
            row.actions
        );
    }

    /// The Wave Nanos are candidates, without RouterOS and with the router they are cabled to.
    #[test]
    fn bridges_without_routeros_are_candidates() {
        let lab = Lab::new();
        let np1_port = lab.np1_port;
        let topology = Arc::new(lab.builder.build());
        let candidates = radio_bridge::candidates(&topology);
        let found = candidates
            .iter()
            .map(|c| {
                (
                    c.device.name.to_string(),
                    c.routeros,
                    c.peer.as_ref().map(|p| p.name.to_string()),
                    c.router_port.as_ref().map(|p| p.id),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(found[0], ("wave-a".into(), false, Some("wave-b".into()), Some(np1_port.0)));
        assert_eq!(found.len(), 2, "{found:?}");
    }
}
