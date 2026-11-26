//! Scope: Tests of the page „Zustand“'s findings (`condition::build`); nothing else.

use super::*;
use crate::{
    device::{
        metrics::{Capsman, Lease, WlanClient},
        poe::{EthernetPort, PoePorts, monitor_reading},
        uplink::UplinkPing,
    },
    network_state::device_data::Resources,
    topology::{
        Cable, CablePort, Device, Interface, InterfaceId, IpAddressData, IpAddressId,
        PhysicalPortId, PoeMode, PortType, Topology, builder::TopologyBuilder,
    },
};
use std::{str::FromStr, sync::Arc};

fn address(builder: &mut TopologyBuilder, ip: &str) -> IpAddressId {
    let id = builder.ip_addresses.next_id();
    builder.ip_addresses.insert(
        id,
        IpAddressData {
            ip: ip.parse().unwrap(),
            interface: None,
            prefix: None,
        },
    );
    id
}

fn router(builder: &mut TopologyBuilder, name: &str, loopback: &str) -> DeviceId {
    let primary = Some(address(builder, loopback));
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            primary_ip_v4: primary,
            loopback_ip: primary,
            ..Device::default()
        },
    );
    id
}

fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: &str) -> InterfaceId {
    let ips = [address(builder, ip)].into();
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            ips,
            use_ospf: true,
            ..Interface::default()
        },
    );
    id
}

fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
    let id = builder.cables.next_id();
    builder.cables.insert(
        id,
        Cable {
            port_a: Box::new([CablePort::Interface(a)]),
            port_b: Box::new([CablePort::Interface(b)]),
            kind: None,
        },
    );
}

/// The routers core, a and b: core's `ether1` cabled to a, `ether2` to b and feeding it (`pse`,
/// b's port `pd`); core has its uplink on `ether3`.
struct Net {
    devices: Vec<DeviceAccess>,
    _topology: Arc<Topology>,
    to_a: [InterfaceId; 2],
    to_b: [InterfaceId; 2],
}

fn net() -> Net {
    let mut builder = TopologyBuilder::default();
    let core = router(&mut builder, "core", "172.16.1.1/32");
    let a = router(&mut builder, "a", "172.16.1.2/32");
    let b = router(&mut builder, "b", "172.16.1.3/32");
    let core_a = port(&mut builder, core, "ether1", "172.17.1.1/30");
    let a_core = port(&mut builder, a, "ether1", "172.17.1.2/30");
    cable(&mut builder, core_a, a_core);
    let core_b = port(&mut builder, core, "ether2", "172.17.1.5/30");
    let b_core = port(&mut builder, b, "ether1", "172.17.1.6/30");
    builder.interfaces.get_mut(&core_b).unwrap().poe = Some(PoeMode::Pse);
    builder.interfaces.get_mut(&b_core).unwrap().poe = Some(PoeMode::Pd);
    cable(&mut builder, core_b, b_core);
    let uplink = port(&mut builder, core, "ether3", "100.64.0.2/29");
    builder.interfaces.get_mut(&uplink).unwrap().internet = true;
    builder.interfaces.get_mut(&uplink).unwrap().use_ospf = false;
    let topology = Arc::new(builder.build());
    let devices = [core, a, b]
        .map(|id| topology.get_device_by_id(&id).unwrap())
        .to_vec();
    Net {
        devices,
        _topology: topology,
        to_a: [core_a, a_core],
        to_b: [core_b, b_core],
    }
}

fn up_since(uptime: Duration) -> DeviceMapData {
    DeviceMapData {
        resources: Some(Resources {
            uptime,
            version: "7.24.6 (long-term)".into(),
            ..Resources::default()
        }),
        ..DeviceMapData::default()
    }
}

fn readings(device_data: BTreeMap<DeviceId, DeviceMapData>, uplinks: Uplinks) -> Readings {
    Readings {
        device_data,
        uplinks,
        ..Readings::default()
    }
}

const THRESHOLDS: SignalThresholds = SignalThresholds {
    weak: -75,
    bad: -80,
};

const LONG: Duration = Duration::from_secs(30 * 24 * 3600);

fn edge(ends: [InterfaceId; 2], state: EdgeState) -> Edge {
    Edge {
        kind: EdgeKind::Cable,
        a: ends[0],
        b: ends[1],
        state,
    }
}

fn kinds(condition: &NetworkCondition) -> Vec<(ConditionKind, &str)> {
    condition
        .findings
        .iter()
        .map(|finding| (finding.kind, &*finding.device.name))
        .collect()
}

/// All up for long, the links up in both OSPF, the uplink answering every ping: nothing to do.
#[test]
fn a_healthy_network_has_no_findings() {
    let net = net();
    let data = net
        .devices
        .iter()
        .map(|device| (device.id(), up_since(LONG)))
        .collect();
    let edges = [edge(net.to_a, EdgeState::Up), edge(net.to_b, EdgeState::Up)];
    let ping = UplinkPing {
        gateway: [100, 64, 0, 1].into(),
        sent: 5,
        received: 5,
        rtt: Some(Duration::from_millis(2)),
    };
    let uplinks = Uplinks::from([(net.devices[0].id(), UplinkReading::Pinged(ping))]);
    let condition = build(&net.devices, &edges, &readings(data, uplinks), THRESHOLDS);
    assert_eq!(kinds(&condition), vec![]);
    assert_eq!(condition.devices.len(), 3);
}

/// a doesn't answer: one finding for it, not also for its link, which is down for want of it;
/// b's link is up only in OSPFv2, b booted an hour ago and draws nothing from core's port;
/// core's uplink lost a ping. Most urgent first.
#[test]
fn the_findings_most_urgent_first() {
    let net = net();
    let [core, a, b] = [0, 1, 2].map(|i| net.devices[i].id());
    let mut feeding = up_since(LONG);
    feeding.poe = PoePorts {
        ethernet: vec![EthernetPort {
            name: "ether2".into(),
            default_name: "ether2".into(),
            poe_out: true,
        }]
        .into(),
        monitor: vec![monitor_reading(&[
            ("name", "ether2"),
            ("poe-out-status", "waiting-for-load"),
        ])]
        .into(),
    };
    let data = BTreeMap::from([(core, feeding), (b, up_since(Duration::from_secs(3600)))]);
    let edges = [
        edge(net.to_a, EdgeState::Down(DownReason::RouterMissing(a))),
        edge(net.to_b, EdgeState::V3Down),
    ];
    let ping = UplinkPing {
        gateway: [100, 64, 0, 1].into(),
        sent: 5,
        received: 4,
        rtt: Some(Duration::from_millis(12)),
    };
    let uplinks = Uplinks::from([(core, UplinkReading::Pinged(ping))]);
    let condition = build(&net.devices, &edges, &readings(data, uplinks), THRESHOLDS);
    assert_eq!(
        kinds(&condition),
        vec![
            (ConditionKind::Unreachable, "a"),
            (ConditionKind::LinkIpv6Down, "core"),
            (ConditionKind::UplinkLoss, "core"),
            (ConditionKind::Rebooted, "b"),
            (ConditionKind::PoeNoLoad, "core"),
        ]
    );
    let finding = |kind| {
        condition
            .findings
            .iter()
            .find(|finding| finding.kind == kind)
            .unwrap()
    };
    let link = finding(ConditionKind::LinkIpv6Down).link.as_ref().unwrap();
    assert_eq!([&*link.a.device.name, &*link.b.device.name], ["core", "b"]);
    assert_eq!(
        finding(ConditionKind::UplinkLoss).uplink,
        Some(UplinkQuality {
            gateway: "100.64.0.1".into(),
            loss: 0.2,
            rtt_ms: Some(12.0),
        })
    );
    assert_eq!(
        finding(ConditionKind::PoeNoLoad)
            .port
            .as_ref()
            .map(|p| &*p.name),
        Some("ether2")
    );
    // health of those that answered
    assert_eq!(condition.devices.len(), 2);
}

/// A link down with both routers there is a dead link; an uplink router without a default
/// route has no uplink.
#[test]
fn a_dead_link_and_no_default_route() {
    let net = net();
    let data = net
        .devices
        .iter()
        .map(|device| (device.id(), up_since(LONG)))
        .collect();
    let edges = [
        edge(net.to_a, EdgeState::Down(DownReason::NoSegment)),
        edge(net.to_b, EdgeState::Up),
    ];
    let uplinks = Uplinks::from([(net.devices[0].id(), UplinkReading::NoDefaultRoute)]);
    let condition = build(&net.devices, &edges, &readings(data, uplinks), THRESHOLDS);
    assert_eq!(
        kinds(&condition),
        vec![
            (ConditionKind::LinkDown, "core"),
            (ConditionKind::NoDefaultRoute, "core"),
        ]
    );
}

/// A default route out an interface (PPPoE) has nothing to ping: no finding.
#[test]
fn a_default_route_without_an_address_is_no_finding() {
    let net = net();
    let data = net
        .devices
        .iter()
        .map(|device| (device.id(), up_since(LONG)))
        .collect();
    let edges = [edge(net.to_a, EdgeState::Up), edge(net.to_b, EdgeState::Up)];
    let reading = UplinkReading::NoAddress {
        interface: "pppoe-out1".into(),
    };
    let uplinks = Uplinks::from([(net.devices[0].id(), reading)]);
    let condition = build(&net.devices, &edges, &readings(data, uplinks), THRESHOLDS);
    assert_eq!(kinds(&condition), vec![]);
}

fn wlan_client(mac: &str, signal: i16) -> WlanClient {
    WlanClient {
        mac_address: mac.parse().unwrap(),
        interface: "cap-a-1".into(),
        ssid: "berg".into(),
        ap: "a".into(),
        capsman: Capsman::Wifi,
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
        signal,
        signal_to_noise: None,
        uptime: LONG,
        band: None,
    }
}

/// Below -75 dBm a client is weak, below -80 bad, at -75 still neither; a lease names it.
#[test]
fn a_client_s_signal_by_the_thresholds() {
    let net = net();
    let core = net.devices[0].id();
    let device_data = net
        .devices
        .iter()
        .map(|device| (device.id(), up_since(LONG)))
        .collect();
    let edges = [edge(net.to_a, EdgeState::Up), edge(net.to_b, EdgeState::Up)];
    let readings = Readings {
        device_data,
        clients: BTreeMap::from([(
            core,
            Box::from([
                wlan_client("02:00:00:00:00:01", -75),
                wlan_client("02:00:00:00:00:02", -76),
                wlan_client("02:00:00:00:00:03", -85),
            ]),
        )]),
        leases: Box::new([Lease {
            server: "dhcp-clients".into(),
            address: "10.0.1.20".parse().unwrap(),
            mac_address: "02:00:00:00:00:03".parse().unwrap(),
            hostname: Some("laptop".into()),
        }]),
        uplinks: Uplinks::new(),
    };
    let condition = build(&net.devices, &edges, &readings, THRESHOLDS);
    assert_eq!(
        kinds(&condition),
        vec![
            (ConditionKind::SignalBad, "core"),
            (ConditionKind::SignalWeak, "core"),
        ]
    );
    let bad = condition.findings[0].client.as_ref().unwrap();
    assert_eq!(
        (&*bad.mac_address, bad.hostname.as_deref(), bad.signal),
        ("02:00:00:00:00:03", Some("laptop"), -85)
    );
    let custom = SignalThresholds {
        weak: -70,
        bad: -76,
    };
    let condition = build(&net.devices, &edges, &readings, custom);
    assert_eq!(condition.findings.len(), 3);
}

/// A port NetBox has feed without PoE out, and one feeding without a mode in NetBox: each a
/// PoE finding with what it contradicts (E8).
#[test]
fn a_poe_port_contradicting_netbox() {
    let net = net();
    let core = net.devices[0].id();
    let mut device_data = net
        .devices
        .iter()
        .map(|device| (device.id(), up_since(LONG)))
        .collect::<BTreeMap<_, _>>();
    let port = |name: &str, poe_out| EthernetPort {
        name: name.into(),
        default_name: name.into(),
        poe_out,
    };
    device_data.get_mut(&core).unwrap().poe = PoePorts {
        ethernet: vec![port("ether1", true), port("ether2", false)].into(),
        monitor: vec![
            monitor_reading(&[("name", "ether1"), ("poe-out-status", "powered-on")]),
            monitor_reading(&[("name", "ether2"), ("poe-out-status", "disabled")]),
        ]
        .into(),
    };
    let edges = [edge(net.to_a, EdgeState::Up), edge(net.to_b, EdgeState::Up)];
    let readings = Readings {
        device_data,
        ..Readings::default()
    };
    let condition = build(&net.devices, &edges, &readings, THRESHOLDS);
    let poe = condition
        .findings
        .iter()
        .map(|finding| {
            (
                finding.kind,
                finding.port.as_ref().map(|port| &*port.name),
                finding.poe,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        poe,
        [
            (
                ConditionKind::PoeMismatch,
                Some("ether1"),
                Some(PoeProblem::Undocumented)
            ),
            (
                ConditionKind::PoeMismatch,
                Some("ether2"),
                Some(PoeProblem::NoPoeOut)
            ),
        ]
    );
}
