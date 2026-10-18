//! The watchdogs a device gets from where it is on the way to the management roots.
use crate::{
    device::ros::{
        BaseDeviceDataTarget,
        l2::KeepNameGenerator,
        warning::{ApplyWarning, WatchTargetSilent},
        watchdog::*,
    },
    topology::{
        Cable, CablePort, Device, DeviceId, Interface, InterfaceId, IpAddressData, PhysicalPortId,
        PoeMode, PortType, TopologyHolder, management::ManagementTree, test::TopologyBuilder,
    },
};
use mikrotik_model::ascii::AsciiString;
use std::{net::IpAddr, str::FromStr};

/// `core` (announcing the default route) -- `np` -- radio bridge `ap` ~ `st` -- `far`. `np`
/// powers `ap` and `far` powers `st`; the bridge's segment 10.0.1.0/29 has `np`'s and
/// `far`'s port and both bridges, as `helper::radio_bridge` makes it.
struct Net {
    builder: TopologyBuilder,
    core: DeviceId,
    np: DeviceId,
    ap: DeviceId,
    st: DeviceId,
    far: DeviceId,
}

impl Net {
    fn new() -> Net {
        let mut builder = TopologyBuilder::default();
        let core = router(&mut builder, "core", "172.16.1.1/32");
        let np = router(&mut builder, "np", "172.16.1.2/32");
        let ap = router(&mut builder, "ap", "172.16.1.3/32");
        let st = router(&mut builder, "st", "172.16.1.4/32");
        let far = router(&mut builder, "far", "172.16.1.5/32");
        builder.devices.get_mut(&core).unwrap().propagate_default_route = true;
        let core_e2 = port(&mut builder, core, "ether2", "10.0.0.1/29");
        let np_e1 = port(&mut builder, np, "ether1", "10.0.0.2/29");
        cable(&mut builder, core_e2, np_e1);
        let np_e2 = port(&mut builder, np, "ether2", "10.0.1.1/29");
        let ap_bridge = port(&mut builder, ap, "bridge", "10.0.1.2/29");
        let ap_e1 = bridge_port(&mut builder, ap, "ether1", ap_bridge);
        cable(&mut builder, np_e2, ap_e1);
        powering(&mut builder, np_e2, ap_e1);
        let st_bridge = port(&mut builder, st, "bridge", "10.0.1.3/29");
        let st_e1 = bridge_port(&mut builder, st, "ether1", st_bridge);
        let far_e1 = port(&mut builder, far, "ether1", "10.0.1.4/29");
        cable(&mut builder, far_e1, st_e1);
        powering(&mut builder, far_e1, st_e1);
        Net {
            builder,
            core,
            np,
            ap,
            st,
            far,
        }
    }

    /// The watch address of each device, and the PoE ping of `np`'s and `far`'s feeding ports.
    async fn watchdogs(self, proxy: Option<&str>) -> Watchdogs {
        let ids = [self.core, self.np, self.ap, self.st, self.far];
        let topology: TopologyHolder = self.builder.into();
        let mut watched = Vec::new();
        let mut poe = Vec::new();
        for id in ids {
            let device = topology.devices_by_id(id).await.unwrap();
            let tree = ManagementTree::of(device.topology(), proxy);
            let mut target = BaseDeviceDataTarget::new(b"CRS318-16P-2S+").unwrap();
            target.generate_from(&device, KeepNameGenerator).unwrap();
            target.setup_watchdog(&device, &tree).unwrap();
            assert!(target.watchdog.watchdog_timer);
            assert_eq!(target.watchdog.ping_timeout, PING_TIMEOUT);
            watched.push(target.watchdog.watch_address.value().copied());
            let port = |name: &str| {
                let port = &target.ethernet[&AsciiString::from(name)];
                (
                    port.power_cycle_ping_enabled,
                    port.power_cycle_ping_address,
                    port.power_cycle_ping_timeout,
                )
            };
            poe.push((port("ether1"), port("ether2")));
        }
        Watchdogs { watched, poe }
    }
}

struct Watchdogs {
    /// core, np, ap, st, far
    watched: Vec<Option<IpAddr>>,
    poe: Vec<(PoeSettings, PoeSettings)>,
}

type PoeSettings = (Option<bool>, Option<IpAddr>, Option<std::time::Duration>);

fn ip(address: &str) -> Option<IpAddr> {
    Some(address.parse().unwrap())
}

#[tokio::test]
async fn each_device_watches_its_parent_unless_it_powers_the_way() {
    let watchdogs = Net::new().watchdogs(None).await;
    assert_eq!(
        watchdogs.watched,
        [
            // the root watches nothing
            None,
            ip("172.16.1.1"),
            ip("172.16.1.2"),
            // the station across the radio to np, not through the bridges' loopbacks
            ip("172.16.1.2"),
            // far reaches np only through st, which it powers
            None,
        ]
    );
}

/// At the address at the cable, which the initial setup gives it: its loopback only comes with
/// the first apply.
#[tokio::test]
async fn a_port_powering_a_routeros_device_power_cycles_it() {
    let watchdogs = Net::new().watchdogs(None).await;
    let off = (Some(false), None, None);
    // np: ether1 to core powers nothing, ether2 powers ap
    assert_eq!(
        watchdogs.poe[1],
        (off, (Some(true), ip("10.0.1.2"), Some(POE_PING_TIMEOUT)))
    );
    // far: ether1 powers st
    assert_eq!(
        watchdogs.poe[4].0,
        (Some(true), ip("10.0.1.3"), Some(POE_PING_TIMEOUT))
    );
    assert_eq!(watchdogs.poe[0], (off, off));
}

/// Without a device announcing the default route, the management proxy's is the root; without
/// either nothing is watched.
#[tokio::test]
async fn the_management_proxy_is_a_root() {
    let without_default = || {
        let mut net = Net::new();
        net.builder.devices.get_mut(&net.core).unwrap().propagate_default_route = false;
        net
    };
    assert_eq!(
        without_default().watchdogs(Some("np")).await.watched,
        [ip("172.16.1.2"), None, ip("172.16.1.2"), ip("172.16.1.2"), None]
    );
    assert_eq!(without_default().watchdogs(None).await.watched, [None; 5]);
}

#[tokio::test]
async fn the_tree_knows_the_children() {
    let net = Net::new();
    let (core, np) = (net.core, net.np);
    let topology: TopologyHolder = net.builder.into();
    let device = topology.devices_by_id(core).await.unwrap();
    let tree = ManagementTree::of(device.topology(), None);
    assert!(tree.is_root(core));
    assert_eq!(tree.parent(core), None);
    assert_eq!(tree.children(core).collect::<Vec<_>>(), [np]);
    assert_eq!(tree.children(np).count(), 3);
}

/// The AP of the radio bridge being away cuts the station and the far router off from `np`,
/// which they watch through it; `np` being away cuts off its children.
#[tokio::test]
async fn the_devices_on_the_far_side_depend_on_the_bridge() {
    let net = Net::new();
    let (core, np, ap, st, far) = (net.core, net.np, net.ap, net.st, net.far);
    let topology: TopologyHolder = net.builder.into();
    let device = topology.devices_by_id(core).await.unwrap();
    let tree = ManagementTree::of(device.topology(), None);
    assert_eq!(tree.depending_on(ap), [st, far].into());
    assert_eq!(tree.depending_on(st), [ap, far].into());
    assert_eq!(tree.depending_on(np), [ap, st, far].into());
    assert_eq!(tree.depending_on(core), [np].into());
    // NetBox doesn't tell which way through a segment goes: more suspended than needed
    assert_eq!(tree.depending_on(far), [ap, st].into());
}

fn router(builder: &mut TopologyBuilder, name: &str, loopback: &str) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            ..Device::default()
        },
    );
    let address = address(builder, loopback);
    let lo = builder.interfaces.next_id();
    builder.interfaces.insert(
        lo,
        Interface {
            name: "lo".into(),
            device: id,
            external: Some(PhysicalPortId::Loopback),
            port_type: Some(PortType::Loopback),
            ips: Box::new([address]),
            ..Interface::default()
        },
    );
    let device = builder.devices.get_mut(&id).unwrap();
    device.primary_ip_v4 = Some(address);
    device.loopback_ip = Some(address);
    id
}

/// An `ospf` port (or bridge, by its name) with `ip`.
fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: &str) -> InterfaceId {
    let address = address(builder, ip);
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(if name == "bridge" {
                PortType::Bridge
            } else {
                PortType::Ethernet
            }),
            ips: Box::new([address]),
            use_ospf: true,
            ..Interface::default()
        },
    );
    id
}

fn bridge_port(
    builder: &mut TopologyBuilder,
    device: DeviceId,
    name: &str,
    bridge: InterfaceId,
) -> InterfaceId {
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            bridge: Some(bridge),
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

fn powering(builder: &mut TopologyBuilder, pse: InterfaceId, pd: InterfaceId) {
    builder.interfaces.get_mut(&pse).unwrap().poe = Some(PoeMode::Pse);
    builder.interfaces.get_mut(&pd).unwrap().poe = Some(PoeMode::Pd);
}

fn address(builder: &mut TopologyBuilder, ip: &str) -> crate::topology::IpAddressId {
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

/// A silent address keeps what the device has: on a device that watches nothing yet, nothing
/// starts, and each silent address is a warning.
#[tokio::test]
async fn a_silent_address_keeps_what_the_device_has() {
    let net = Net::new();
    let np = net.np;
    let topology: TopologyHolder = net.builder.into();
    let device = topology.devices_by_id(np).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS318-16P-2S+").unwrap();
    target.generate_from(&device, KeepNameGenerator).unwrap();
    let mut current = crate::device::ros::test::empty_current(&target);
    // the factory's: no power cycling
    for port in &mut current.ethernet {
        port.data.power_cycle_ping_enabled = Some(false);
        port.data.power_cycle_ping_address = None;
        port.data.power_cycle_ping_timeout = None;
    }
    assert_eq!(
        target.watched_addresses(),
        [ip("172.16.1.1").unwrap(), ip("10.0.1.2").unwrap()]
    );
    let answering = [ip("172.16.1.1").unwrap()].into();
    let warnings = target.keep_silent_watchdogs(&current, &answering, |_| "ap".into());
    assert_eq!(target.watchdog.watch_address.value().copied(), ip("172.16.1.1"));
    let ether2 = &target.ethernet[&AsciiString::from("ether2")];
    assert_eq!(ether2.power_cycle_ping_enabled, Some(false));
    assert_eq!(ether2.power_cycle_ping_address, None);
    assert_eq!(
        warnings,
        [ApplyWarning::WatchTargetSilent(WatchTargetSilent {
            address: "10.0.1.2".into(),
            port: Some("ether2".into()),
            device: "ap".into(),
        })]
    );
    // nothing answers: the device keeps watching nothing
    let warnings = target.keep_silent_watchdogs(&current, &Default::default(), |_| "".into());
    assert_eq!(target.watchdog.watch_address.value(), None);
    assert_eq!(warnings.len(), 1);
}

/// Of two neighbors as near to a root, the one cabled to the device is its parent, not the
/// one across a radio bridge (the lab's lab-sxt-b and lab-np2).
#[tokio::test]
async fn the_cabled_neighbor_is_the_parent() {
    let mut net = Net::new();
    // a second router next to core, cabled to st, in the bridge's segment
    let np2 = router(&mut net.builder, "aa-np", "172.16.1.6/32");
    let core_e3 = port(&mut net.builder, net.core, "ether3", "10.0.2.1/29");
    let np2_e1 = port(&mut net.builder, np2, "ether1", "10.0.2.2/29");
    cable(&mut net.builder, core_e3, np2_e1);
    let np2_e2 = port(&mut net.builder, np2, "ether2", "10.0.1.6/29");
    let st_e2 = net.builder.interfaces.next_id();
    let st_bridge = net
        .builder
        .interfaces
        .iter()
        .find(|(_, i)| i.device == net.st && i.name.as_ref() == "bridge")
        .map(|(id, _)| *id);
    net.builder.interfaces.insert(
        st_e2,
        Interface {
            name: "ether2".into(),
            device: net.st,
            external: Some(PhysicalPortId::Ethernet(2)),
            port_type: Some(PortType::Ethernet),
            bridge: st_bridge,
            ..Interface::default()
        },
    );
    cable(&mut net.builder, np2_e2, st_e2);
    // `aa-np` comes first by name, `np` is cabled to ap
    assert_eq!(
        net.watchdogs(None).await.watched,
        [None, ip("172.16.1.1"), ip("172.16.1.2"), ip("172.16.1.6"), None]
    );
}
