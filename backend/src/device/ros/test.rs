use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, GapFinder, pool_range};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use mikrotik_model::model::{InterfaceEthernetByDefaultName, InterfaceEthernetCfg};
use std::str::FromStr;

/// RouterOS rejects `auto-link-local=no` on anything but a link-local address.
#[test]
pub fn test_auto_link_local_only_off_for_link_local() {
    let mut target = BaseDeviceDataTarget::new(b"CRS326-24G-2S+").unwrap();
    let global: Ipv6Net = "2a02:16a:fe00:102::1/64".parse().unwrap();
    let link_local: Ipv6Net = "fe80::1/64".parse().unwrap();
    target.set_ip_address(IpNet::V6(global), "ether2");
    target.set_ip_address(IpNet::V6(link_local), "ether2");
    assert!(target.ipv_6_address[&global].0.auto_link_local);
    assert!(!target.ipv_6_address[&link_local].0.auto_link_local);
}

#[test]
pub fn test_gap_empty() {
    let finder = GapFinder::new();
    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec!["172.16.1.1".parse().unwrap().."172.16.1.254".parse().unwrap()]
    );
}
#[test]
pub fn test_gap_none() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4_net("172.16.1.0/24".parse().unwrap());
    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(gaps, vec![]);
}
#[test]
pub fn test_gap_router_ip() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4("172.16.1.1".parse().unwrap());
    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec!["172.16.1.2".parse().unwrap().."172.16.1.254".parse().unwrap()]
    );
}
#[test]
pub fn test_gap_router_and_host_ip() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4("172.16.1.1".parse().unwrap());
    finder.reserve_ipv4("172.16.1.10".parse().unwrap());
    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec![
            "172.16.1.2".parse().unwrap().."172.16.1.9".parse().unwrap(),
            "172.16.1.11".parse().unwrap().."172.16.1.254".parse().unwrap()
        ]
    );
}
#[test]
pub fn test_gap_overlapping_ranges() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4("172.16.1.1".parse().unwrap());
    finder.reserve_ipv4("172.16.1.15".parse().unwrap());
    finder.reserve_ipv4_range("172.16.1.10".parse().unwrap().."172.16.1.20".parse().unwrap());

    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec![
            "172.16.1.2".parse().unwrap().."172.16.1.9".parse().unwrap(),
            "172.16.1.21".parse().unwrap().."172.16.1.254".parse().unwrap()
        ]
    );
}
#[test]
pub fn test_gap_subnet() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4("172.16.1.1".parse().unwrap());
    finder.reserve_ipv4("172.16.1.15".parse().unwrap());
    finder.reserve_ipv4_range("172.16.1.10".parse().unwrap().."172.16.1.20".parse().unwrap());
    finder.reserve_ipv4_net(Ipv4Net::from_str("172.16.3.0/24").unwrap());

    let gaps = finder
        .find_gaps_ipv4("172.16.0.0/12".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec![
            "172.16.0.1".parse().unwrap().."172.16.1.0".parse().unwrap(),
            "172.16.1.2".parse().unwrap().."172.16.1.9".parse().unwrap(),
            "172.16.1.21".parse().unwrap().."172.16.2.255".parse().unwrap(),
            "172.16.4.0".parse().unwrap().."172.31.255.254".parse().unwrap()
        ]
    );
}

/// A /30 with the gateway leaves one address for DHCP, and a single free address between two
/// reserved ones is a gap too: such a pool was lost, and with it the DHCP server.
#[test]
pub fn test_gap_single_addresses() {
    let mut finder = GapFinder::new();
    finder.reserve_ipv4("10.200.1.9".parse().unwrap());
    let gaps = finder
        .find_gaps_ipv4("10.200.1.8/30".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(gaps, vec!["10.200.1.10".parse().unwrap().."10.200.1.10".parse().unwrap()]);

    let mut finder = GapFinder::new();
    for ip in ["172.16.1.1", "172.16.1.3", "172.16.1.254"] {
        finder.reserve_ipv4(ip.parse().unwrap());
    }
    let gaps = finder
        .find_gaps_ipv4("172.16.1.0/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        gaps,
        vec![
            "172.16.1.2".parse().unwrap().."172.16.1.2".parse().unwrap(),
            "172.16.1.4".parse().unwrap().."172.16.1.253".parse().unwrap(),
        ]
    );
}

/// What CHR 7.24.5 reported for these ranges written as `start-end`.
#[test]
pub fn test_pool_range_as_routeros_reports_it() {
    for (start, end, reported) in [
        ("10.200.1.10", "10.200.1.10", "10.200.1.10"),
        ("10.200.1.20", "10.200.1.21", "10.200.1.20/31"),
        ("10.1.0.2", "10.1.0.254", "10.1.0.2-10.1.0.254"),
        ("10.1.1.128", "10.1.1.255", "10.1.1.128/25"),
        ("10.1.2.1", "10.1.2.2", "10.1.2.1-10.1.2.2"),
        ("10.1.3.0", "10.1.4.255", "10.1.3.0-10.1.4.255"),
        ("10.1.8.4", "10.1.8.7", "10.1.8.4/30"),
    ] {
        assert_eq!(
            pool_range(&(start.parse().unwrap()..end.parse().unwrap())),
            reported
        );
    }
}

/// The state of a freshly reset device with the same ports as `target`: every
/// port under its default name, nothing else configured.
pub(crate) fn empty_current(target: &BaseDeviceDataTarget) -> BaseDeviceDataCurrent {
    BaseDeviceDataCurrent {
        ospf_interface: Box::new([]),
        interface_list: Box::new([]),
        identity: Default::default(),
        bridge: Box::new([]),
        bridge_port: Box::new([]),
        ethernet: target
            .ethernet
            .iter()
            .map(|(default_name, e)| InterfaceEthernetByDefaultName {
                default_name: default_name.clone(),
                data: InterfaceEthernetCfg {
                    name: default_name.clone(),
                    ..e.clone()
                },
            })
            .collect(),
        ipv_6_address: Box::new([]),
        ospf_instance: Box::new([]),
        vxlan: Box::new([]),
        ospf_area: Box::new([]),
        vxlan_vteps: Box::new([]),
        interface_list_member: Box::new([]),
        bridge_vlan: Box::new([]),
        ipv_6_firewall_address_list: Box::new([]),
        ipv_4_address: Box::new([]),
        vlan: Box::new([]),
        dhcp_v_4_client: Box::new([]),
        ipv_6_firewall_filter: Box::new([]),
        ip_firewall_filter: Box::new([]),
        ip_firewall_nat: Box::new([]),
        dns: Default::default(),
        ipv_6_nd: Box::new([]),
        dhcp_v_4_server: Box::new([]),
        dhcp_v_4_server_network: Box::new([]),
        ipv_4_pool: Box::new([]),
        socks: Default::default(),
        socks_users: Box::new([]),
        ssh: Default::default(),
        ipv_6_settings: Default::default(),
        mac_server: Default::default(),
        neighbor_discovery: Default::default(),
        watchdog: Default::default(),
    }
}
