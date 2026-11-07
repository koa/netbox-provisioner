//! Scope: Tests of a DHCP server's pool; nothing else.
use crate::device::ros::dhcp::{GapFinder, pool_range};
use ipnet::Ipv4Net;
use std::str::FromStr;

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
    assert_eq!(
        gaps,
        vec!["10.200.1.10".parse().unwrap().."10.200.1.10".parse().unwrap()]
    );

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
