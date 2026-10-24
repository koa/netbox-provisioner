//! The pool of a DHCP server: the addresses of a prefix nothing else takes.

use crate::topology::access::ip_prefix::IpPrefixAccess;
use ipnet::{IpNet, Ipv4Net};
use mikrotik_model::MacAddress;
use std::{
    net::{IpAddr, Ipv4Addr},
    ops::{Range, RangeInclusive},
};

/// The ranges a DHCP server on `net` (of `prefix`) hands out: the prefix' IP ranges marked
/// DHCP, else every address of it no address, IP range or child prefix takes. Empty, there is
/// no DHCP server (finding `DhcpPoolEmpty`), as on a /32 without prefix.
pub(crate) fn dhcp_pool_v4(prefix: &IpPrefixAccess, net: Ipv4Net) -> Vec<Range<Ipv4Addr>> {
    let mut explicit = Vec::new();
    let mut gap_finder = GapFinder::new();
    for range in prefix.ranges() {
        if let (Some(IpAddr::V4(start)), Some(IpAddr::V4(end))) = (range.start(), range.end()) {
            if range.is_dhcp() {
                explicit.push(start..end);
            }
            gap_finder.reserve_ipv4_range(start..end);
        }
    }
    if !explicit.is_empty() {
        return explicit;
    }
    for child_prefix in prefix.children() {
        if let Some(IpNet::V4(net)) = child_prefix.prefix() {
            gap_finder.reserve_ipv4_net(net);
        }
    }
    for ip in prefix.ips() {
        if let Some(IpAddr::V4(ip)) = ip.addr() {
            gap_finder.reserve_ipv4(ip)
        }
    }
    gap_finder.find_gaps_ipv4(net).collect()
}

/// A static lease of a DHCP server: a NetBox device that isn't a RouterOS one, with an
/// interface that has a MAC and an IPv4 address of the server's prefix (`docs/netbox-modeling.md`,
/// „Statische DHCP-Leases“).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StaticLease {
    pub address: Ipv4Addr,
    pub mac: MacAddress,
    /// the client device's name, the lease's comment
    pub name: Box<str>,
}

/// The static leases in `prefix` for its DHCP server, by address (the router itself runs
/// RouterOS, so its own address is none).
pub(crate) fn static_leases(prefix: &IpPrefixAccess) -> Vec<StaticLease> {
    let mut leases = prefix
        .ips()
        .iter()
        .filter_map(|ip| {
            let Some(IpAddr::V4(address)) = ip.addr() else { return None };
            let interface = ip.interface()?;
            let device = interface.device()?;
            if device.has_routeros() {
                return None;
            }
            Some(StaticLease {
                address,
                mac: interface.mac()?.parse().ok()?,
                name: device.name().into(),
            })
        })
        .collect::<Vec<_>>();
    leases.sort_by_key(|lease| lease.address);
    leases
}

/// A pool range (`end` included) as RouterOS reports it: one address alone, a range covering
/// exactly one network as that network, every other one as `start-end` (CHR 7.24.5 turns
/// `10.200.1.20-10.200.1.21` into `10.200.1.20/31`; written otherwise, the diff never ends).
pub(super) fn pool_range(range: &Range<Ipv4Addr>) -> String {
    let (start, end) = (u32::from(range.start), u32::from(range.end));
    let size = u64::from(end) - u64::from(start) + 1;
    if start == end {
        range.start.to_string()
    } else if size.is_power_of_two() && u64::from(start) % size == 0 {
        format!("{}/{}", range.start, 32 - size.trailing_zeros())
    } else {
        format!("{}-{}", range.start, range.end)
    }
}

/// The addresses of a network not reserved, as ranges with both ends included (as RouterOS'
/// `/ip pool` takes them): reserved are single addresses, IP ranges and child prefixes.
pub(super) struct GapFinder {
    reserved: Vec<RangeInclusive<u32>>,
}

impl GapFinder {
    pub fn new() -> Self {
        GapFinder {
            reserved: Vec::new(),
        }
    }
    pub fn reserve_ipv4(&mut self, ip: Ipv4Addr) {
        self.reserved.push(u32::from(ip)..=u32::from(ip));
    }
    /// `ip.end` included, as in a NetBox IP range
    pub fn reserve_ipv4_range(&mut self, ip: Range<Ipv4Addr>) {
        self.reserved.push(u32::from(ip.start)..=u32::from(ip.end));
    }
    pub fn reserve_ipv4_net(&mut self, ip: Ipv4Net) {
        self.reserve_ipv4_range(ip.network()..ip.broadcast());
    }
    /// The free ranges of `net` without its network and broadcast address, each `end`
    /// included; a single free address is a range from it to itself.
    pub fn find_gaps_ipv4(&self, net: Ipv4Net) -> impl Iterator<Item = Range<Ipv4Addr>> {
        let (first, last) = (
            u64::from(u32::from(net.network())) + 1,
            u64::from(u32::from(net.broadcast())).saturating_sub(1),
        );
        let mut reserved = self.reserved.clone();
        reserved.sort_by_key(|range| *range.start());
        let mut gaps = Vec::new();
        // the first address not known to be reserved
        let mut next = first;
        for range in reserved {
            let (start, end) = (u64::from(*range.start()), u64::from(*range.end()));
            if start > next && next <= last {
                gaps.push(next..(start - 1).min(last));
            }
            next = next.max(end + 1);
        }
        if next <= last {
            gaps.push(next..last);
        }
        gaps.into_iter()
            .map(|gap| Ipv4Addr::from(gap.start as u32)..Ipv4Addr::from(gap.end as u32))
    }
}
