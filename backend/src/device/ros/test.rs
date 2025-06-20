use crate::device::ros::GapFinder;
use ipnet::Ipv4Net;
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
