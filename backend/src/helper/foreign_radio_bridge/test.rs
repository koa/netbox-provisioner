//! Scope: Tests of `foreign_radio_bridge`; nothing else.
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
            [InterfaceTag::Ospf, InterfaceTag::Dhcp].into(),
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
    let Err(error) = lab.plan().await else {
        panic!("two networks of its own")
    };
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
    let Err(error) = lab.plan().await else {
        panic!("mixed")
    };
    assert!(
        matches!(&error, Error::NoRouterOs(name) if &**name == "wave-a"),
        "{error:?}"
    );
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
/// port to one is a radio link the helper is suggested for (its removal once NetBox has the
/// link's networks, as by now). The plan stops at 1og-sw2's
/// port while it is in its client bridge.
#[tokio::test]
async fn the_wave_nanos_of_the_export_pair_their_radios() {
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
    let mut plan = NetboxPlan::new(&ap, None);
    for (device, other, router_port) in [
        (&ap, &station, "switch-schmidrueti ether1"),
        (&station, &ap, "1og-sw2 ether14"),
    ] {
        let cable = radio_bridge::router_cable(&topology, &mut plan, device, other, None)
            .expect("a router port");
        assert_eq!(&*port_name(&cable.router_port), router_port);
    }
    let input = RadioBridgeInput {
        ap: ap.id().0,
        station: station.id().0,
        ap_router_port: None,
        station_router_port: None,
        ap_address: None,
        station_address: None,
        ssid: None,
    };
    // 1og-sw2 ether14 in its client bridge, whichever the export has (a plan that gets as
    // far as the networks asks NetBox for free ones)
    let in_bridge = Arc::new(
        topology
            .rebuilt_with(|json| {
                for device in json["device_list"].as_array_mut().into_iter().flatten() {
                    for interface in device["interfaces"].as_array_mut().into_iter().flatten() {
                        if interface["id"] == "25" {
                            interface["bridge"] = serde_json::json!({ "id": "415" });
                        }
                    }
                }
            })
            .expect("a source")
            .expect("rebuilt"),
    );
    let mut routes = Some(Routes::none("test"));
    let Err(error) = radio_bridge::plan_with(&in_bridge, &input, &mut routes).await else {
        panic!("1og-sw2 ether14 is in its client bridge")
    };
    assert!(
        matches!(&error, Error::PortInBridge { port, bridge }
                if &**port == "1og-sw2 ether14" && &**bridge == "switch-clients"),
        "{error:?}"
    );
    let pairs = pairs(&ap, &station)
        .expect("pairs")
        .iter()
        .map(|pair| (pair.ap.name().to_string(), pair.station.name().to_string()))
        .collect::<Vec<_>>();
    assert_eq!(
        pairs,
        [
            ("wlan0".into(), "wlan0".into()),
            ("wlan1".into(), "wlan1".into())
        ]
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
    // the far side too, its Wave Nano's port by NetBox type
    let path = row.radio_path.as_ref().expect("a radio path");
    let name = |port: &Option<crate::topology::findings::InterfaceRef>| {
        port.as_ref()
            .map(|port| format!("{} {}", port.device.name, port.name))
    };
    assert_eq!(
        name(&path.peer_bridge_port).as_deref(),
        Some("berg-schmidrueti eth0")
    );
    assert_eq!(
        name(&path.peer_router_port).as_deref(),
        Some("1og-sw2 ether14")
    );
    // the helper until the link has its networks, then the removal
    let planned = router
        .interfaces()
        .iter()
        .any(|i| i.name() == "ether1" && !i.ips().is_empty());
    let (kind, helper) = if planned {
        (ActionKind::Remove, ActionHelper::RadioDisconnect)
    } else {
        (ActionKind::Suggestion, ActionHelper::RadioBridge)
    };
    assert!(
        row.actions
            .iter()
            .any(|action| action.kind == kind && action.helper == helper),
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
    assert_eq!(
        found[0],
        (
            "wave-a".into(),
            false,
            Some("wave-b".into()),
            Some(np1_port.0)
        )
    );
    assert_eq!(found.len(), 2, "{found:?}");
}
