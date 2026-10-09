use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget, GapFinder};
use ipnet::Ipv4Net;
use mikrotik_model::model::{InterfaceEthernetByDefaultName, InterfaceEthernetCfg};
use std::{net::Ipv4Addr, str::FromStr};

#[test]
pub fn test_gap_empty() {
    let finder = GapFinder::<Ipv4Addr>::new();
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
    let mut finder = GapFinder::<Ipv4Addr>::new();
    finder.reserve("172.16.1.0".parse().unwrap().."172.16.1.255".parse().unwrap());
    let gaps = finder
        .find_gaps_ipv4("172.16.1.1/24".parse().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(gaps, vec![]);
}
#[test]
pub fn test_gap_router_ip() {
    let mut finder = GapFinder::<Ipv4Addr>::new();
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
    let mut finder = GapFinder::<Ipv4Addr>::new();
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
    let mut finder = GapFinder::<Ipv4Addr>::new();
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
    let mut finder = GapFinder::<Ipv4Addr>::new();
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
        dhcp_v_4_server: Box::new([]),
        dhcp_v_4_server_network: Box::new([]),
        ipv_4_pool: Box::new([]),
    }
}
