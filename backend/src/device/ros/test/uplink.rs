//! The internet uplink (`dhcp-client`, `nat`): its IPv6 DHCP client with prefix delegation
//! (`BaseDeviceDataTarget::enable_dhcp_v6_client`, `keep_dhcp_v6_clients`) and the list
//! `uplink` (`add_to_uplinks`).
use super::empty_current;
use crate::{
    device::ros::{BaseDeviceDataTarget, UPLINK_LIST, l2::KeepNameGenerator},
    topology::{Device, Interface, PhysicalPortId, TopologyHolder, builder::TopologyBuilder},
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{Ipv6DhcpClientByInterface, Ipv6DhcpClientRequest},
};

/// A router with `ether1` the internet uplink (`dhcp-client`, `nat`) and `ether2` a DHCP
/// client into another network (`dhcp-client` only).
async fn uplink_router() -> BaseDeviceDataTarget {
    let mut builder = TopologyBuilder::default();
    let core = builder.devices.next_id();
    builder.devices.insert(
        core,
        Device {
            name: "core".into(),
            has_routeros: true,
            ..Default::default()
        },
    );
    for (number, nat) in [(1, true), (2, false)] {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: format!("ether{number}").into(),
                device: core,
                external: Some(PhysicalPortId::Ethernet(number)),
                enable_dhcp_client: true,
                nat,
                ..Default::default()
            },
        );
    }
    let topology: TopologyHolder = builder.into();
    let core = topology.devices_by_id(core).await.unwrap();
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    target.generate_from(&core, KeepNameGenerator).unwrap();
    target
}

fn names(target: &BaseDeviceDataTarget) -> Vec<String> {
    target.dhcp_v_6_client.keys().map(|name| name.to_string()).collect()
}

/// Only the uplink with `nat` asks for a prefix, into `pd-<port>`, with a default route as
/// its IPv4 client; both ports get an IPv4 client.
#[tokio::test]
async fn the_internet_uplink_asks_for_a_prefix() {
    let target = uplink_router().await;
    assert_eq!(names(&target), ["ether1"]);
    let client = &target.dhcp_v_6_client[&AsciiString::from("ether1")].0;
    assert_eq!(
        client.request,
        [Ipv6DhcpClientRequest::Address, Ipv6DhcpClientRequest::Prefix].into()
    );
    assert_eq!(client.pool_name.as_ref().map(|p| p.to_string()).as_deref(), Some("pd-ether1"));
    assert!(client.add_default_route);
    assert_eq!(client.default_route_distance, Some(10));
    assert_eq!(target.dhcp_v_4_client.len(), 2);
}

/// The device's client at the uplink asks for an address only (as set up by hand): it gets
/// the prefix, keeps its pool's name and script; the client at another port stays as it is.
#[tokio::test]
async fn a_client_set_up_by_hand_gets_prefix_delegation() {
    let mut target = uplink_router().await;
    let mut current = empty_current(&target);
    let own = target.dhcp_v_6_client[&AsciiString::from("ether1")].0.clone();
    let by_hand = |interface: &str| {
        let mut cfg = own.clone();
        cfg.interface = interface.into();
        cfg.request = [Ipv6DhcpClientRequest::Address].into();
        cfg.add_default_route = false;
        cfg.default_route_distance = None;
        cfg.pool_name = Some("fiber7-pool".into());
        cfg.pool_prefix_length = Some(56);
        cfg.script = Some(":log info pd".into());
        Ipv6DhcpClientByInterface(cfg)
    };
    current.dhcp_v_6_client = Box::new([by_hand("ether1"), by_hand("sfp-sfpplus1")]);
    target.keep_dhcp_v6_clients(&current);

    assert_eq!(names(&target), ["ether1", "sfp-sfpplus1"]);
    let uplink = &target.dhcp_v_6_client[&AsciiString::from("ether1")].0;
    assert_eq!(
        uplink.request,
        [Ipv6DhcpClientRequest::Address, Ipv6DhcpClientRequest::Prefix].into()
    );
    assert!(uplink.add_default_route);
    assert_eq!(uplink.default_route_distance, Some(10));
    assert_eq!(uplink.pool_name.as_ref().map(|p| p.to_string()).as_deref(), Some("fiber7-pool"));
    assert_eq!(uplink.pool_prefix_length, Some(56));
    assert!(uplink.script.is_some());
    assert_eq!(
        target.dhcp_v_6_client[&AsciiString::from("sfp-sfpplus1")],
        current.dhcp_v_6_client[1]
    );
}

/// Only the port with `nat` is in the list `uplink`, which hand-made firewall rules use.
#[tokio::test]
async fn the_nat_uplink_is_in_the_list_uplink() {
    let target = uplink_router().await;
    let list = AsciiString::from(UPLINK_LIST);
    assert!(target.interface_list.contains_key(&list));
    let members = target
        .interface_list_member
        .keys()
        .filter(|(_, member_of)| *member_of == list)
        .map(|(interface, _)| interface.to_string())
        .collect::<Vec<_>>();
    assert_eq!(members, ["ether1"]);
}
