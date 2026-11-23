//! Scope: Tests of `auto_rollout::precondition`; nothing else.
use super::*;
use crate::topology::{
    Device, Interface, IpAddressData, PhysicalPortId, PortType, TopologyHolder,
    builder::TopologyBuilder,
};
use std::str::FromStr;

/// A RouterOS device with `loopbacks` on `lo`.
fn router(builder: &mut TopologyBuilder, name: &str, loopbacks: &[&str]) -> DeviceId {
    let id = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: name.into(),
            has_routeros: true,
            ..Device::default()
        },
    );
    let ips = loopbacks
        .iter()
        .map(|ip| address(builder, ip))
        .collect::<Box<[_]>>();
    let lo = builder.interfaces.next_id();
    builder.interfaces.insert(
        lo,
        Interface {
            name: "lo".into(),
            device: id,
            external: Some(PhysicalPortId::Loopback),
            port_type: Some(PortType::Loopback),
            ips,
            ..Interface::default()
        },
    );
    id
}

/// A router with both loopbacks, numbered by `n`.
fn full(builder: &mut TopologyBuilder, name: &str, n: u8) -> DeviceId {
    let v4 = format!("192.168.100.{n}/32");
    let v6 = format!("2a02:16a:fe00:8000::{n}/128");
    router(builder, name, &[&v4, &v6])
}

/// An `ospf` port with `ip`.
fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, ip: &str) {
    let address = address(builder, ip);
    let id = builder.interfaces.next_id();
    builder.interfaces.insert(
        id,
        Interface {
            name: name.into(),
            device,
            external: PhysicalPortId::from_str(name).ok(),
            port_type: Some(PortType::Ethernet),
            ips: Box::new([address]),
            use_ospf: true,
            ..Interface::default()
        },
    );
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

async fn unmet_of(builder: TopologyBuilder, automatic: &[DeviceId]) -> Vec<(Box<str>, Vec<Lack>)> {
    let topology: TopologyHolder = builder.into();
    let mut devices = Vec::new();
    for id in automatic {
        devices.push(topology.devices_by_id(*id).await.unwrap());
    }
    unmet(&devices)
        .into_iter()
        .map(|unmet| (unmet.device.name, unmet.lacks.into_vec()))
        .collect()
}

/// Neighbors with both loopbacks meet it, also over a device outside the automatic sites.
#[tokio::test]
async fn connected_devices_with_loopbacks_meet_it() {
    let mut builder = TopologyBuilder::default();
    let a = full(&mut builder, "a", 1);
    let manual = full(&mut builder, "manual", 2);
    let b = full(&mut builder, "b", 3);
    port(&mut builder, a, "ether1", "10.0.0.0/31");
    port(&mut builder, manual, "ether1", "10.0.0.1/31");
    port(&mut builder, manual, "ether2", "10.0.0.2/31");
    port(&mut builder, b, "ether1", "10.0.0.3/31");
    assert_eq!(unmet_of(builder, &[a, b]).await, []);
}

/// A loopback deleted in NetBox stops the rounds, per family.
#[tokio::test]
async fn a_missing_loopback_is_named_per_family() {
    let mut builder = TopologyBuilder::default();
    let a = full(&mut builder, "a", 1);
    let b = router(&mut builder, "b", &["192.168.100.2/32"]);
    let c = router(&mut builder, "c", &["2a02:16a:fe00:8000::3/128"]);
    port(&mut builder, a, "ether1", "10.0.0.1/24");
    port(&mut builder, b, "ether1", "10.0.0.2/24");
    port(&mut builder, c, "ether1", "10.0.0.3/24");
    assert_eq!(
        unmet_of(builder, &[a, b, c]).await,
        [
            ("b".into(), vec![Lack::LoopbackIpv6]),
            ("c".into(), vec![Lack::LoopbackIpv4])
        ]
    );
}

/// A loopback of another length is none: a `/24` on `lo` is a network, not the router's
/// address.
#[tokio::test]
async fn a_loopback_needs_its_full_length() {
    let mut builder = TopologyBuilder::default();
    let a = router(
        &mut builder,
        "a",
        &["192.168.100.1/24", "2a02:16a:fe00:8000::1/64"],
    );
    assert_eq!(
        unmet_of(builder, &[a]).await,
        [("a".into(), vec![Lack::LoopbackIpv4, Lack::LoopbackIpv6])]
    );
}

/// A device whose last connection was deleted lacks it; the others, the bigger group, don't.
#[tokio::test]
async fn a_device_cut_off_lacks_its_connection() {
    let mut builder = TopologyBuilder::default();
    let a = full(&mut builder, "a", 1);
    let b = full(&mut builder, "b", 2);
    let cut = full(&mut builder, "cut", 3);
    port(&mut builder, a, "ether1", "10.0.0.0/31");
    port(&mut builder, b, "ether1", "10.0.0.1/31");
    port(&mut builder, cut, "ether1", "10.0.1.0/31");
    assert_eq!(
        unmet_of(builder, &[cut, a, b]).await,
        [("cut".into(), vec![Lack::Connection])]
    );
}

/// Of two groups as big, the one with the first name keeps its connection.
#[tokio::test]
async fn at_a_tie_the_first_name_keeps_its_connection() {
    let mut builder = TopologyBuilder::default();
    let b = full(&mut builder, "b", 2);
    let a = full(&mut builder, "a", 1);
    assert_eq!(
        unmet_of(builder, &[b, a]).await,
        [("b".into(), vec![Lack::Connection])]
    );
}

/// A device whose device mode has the scheduler off is named, the others not.
#[tokio::test]
async fn a_device_without_scheduler_is_named() {
    let mut builder = TopologyBuilder::default();
    let b = full(&mut builder, "b", 2);
    let a = full(&mut builder, "a", 1);
    let topology: TopologyHolder = builder.into();
    let devices = [
        topology.devices_by_id(b).await.unwrap(),
        topology.devices_by_id(a).await.unwrap(),
    ];
    let unmet = without_scheduler(&devices, &[b].into());
    assert_eq!(
        unmet
            .iter()
            .map(|unmet| (&*unmet.device.name, &*unmet.lacks))
            .collect::<Vec<_>>(),
        [("b", &[Lack::Scheduler][..])]
    );
}
