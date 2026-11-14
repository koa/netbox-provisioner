//! Scope: Tests of where the device sends router advertisements (`/ipv6/nd`,
//! `BaseDeviceDataTarget::setup_router_advertisements`); not the addresses themselves.
use crate::{
    device::ros::BaseDeviceDataTarget,
    topology::{
        Device, DeviceId, TopologyHolder, access::device::DeviceAccess, builder::TopologyBuilder,
    },
};
use ipnet::{IpNet, Ipv6Net};
use mikrotik_model::ascii::AsciiString;
use std::net::{IpAddr, Ipv6Addr};

const DNS: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x53);

async fn device(dns_servers: &[IpAddr]) -> DeviceAccess {
    let mut builder = TopologyBuilder::default();
    let id: DeviceId = builder.devices.next_id();
    builder.devices.insert(
        id,
        Device {
            name: "router".into(),
            has_routeros: true,
            dns_servers: dns_servers.into(),
            ..Default::default()
        },
    );
    let topology: TopologyHolder = builder.into();
    topology.devices_by_id(id).await.unwrap()
}

/// `(interface, disabled, dns)` of each entry.
fn entries(target: &BaseDeviceDataTarget) -> Vec<(String, bool, Option<Vec<Ipv6Addr>>)> {
    target
        .ipv_6_nd
        .values()
        .map(|nd| {
            (
                nd.0.interface.to_string(),
                nd.0.disabled,
                nd.0.dns.as_ref().map(|dns| dns.to_vec()),
            )
        })
        .collect()
}

/// Only the interface whose prefix is announced (a client network) advertises, with the IPv6
/// DNS servers; a transfer network doesn't, nor does `all`.
#[tokio::test]
async fn only_an_interface_with_an_announced_prefix_advertises() {
    let device = device(&[IpAddr::V6(DNS), "10.0.0.53".parse().unwrap()]).await;
    let mut target = BaseDeviceDataTarget::new(b"RB750Gr3").unwrap();
    let clients: Ipv6Net = "2001:db8:61::1/64".parse().unwrap();
    let transfer: Ipv6Net = "2001:db8:62::/127".parse().unwrap();
    target.set_ip_address(IpNet::V6(clients), "vlan-clients");
    target.set_ip_address(IpNet::V6(transfer), "ether2");
    target.ipv_6_address.get_mut(&clients).unwrap().0.advertise = true;
    target.setup_router_advertisements(&device);
    assert_eq!(
        entries(&target),
        [("vlan-clients".into(), false, Some(vec![DNS]))]
    );
    assert_eq!(
        target.ipv_6_nd[&AsciiString::from("vlan-clients")]
            .0
            .advertise_dns,
        Some(true)
    );
}

/// An AP or switch announcing no prefix sends no advertisement: RouterOS's own entry, which
/// can't be removed, is off.
#[tokio::test]
async fn a_device_announcing_no_prefix_turns_the_own_entry_off() {
    let device = device(&[IpAddr::V6(DNS)]).await;
    let mut target = BaseDeviceDataTarget::new(b"RBwAPG-5HacD2HnD").unwrap();
    let transfer: Ipv6Net = "2001:db8:62::1/127".parse().unwrap();
    target.set_ip_address(IpNet::V6(transfer), "ether1");
    target.setup_router_advertisements(&device);
    assert_eq!(entries(&target), [("all".into(), true, None)]);
}
