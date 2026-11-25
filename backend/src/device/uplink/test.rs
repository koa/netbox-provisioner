//! Scope: Tests of the uplink's gateway and the ping's summary; nothing else.

use super::{UplinkPing, default_gateway, rtt, summary};
use crate::device::{access::Row, answer::parsed};
use mikrotik_model::model::IpRoute;
use std::time::Duration;

/// A route as CHR 7.24.6 prints it, with `fields` over its defaults.
fn route(fields: &[(&str, &str)]) -> IpRoute {
    crate::device::answer::parsed_with(
        fields,
        &[
            (".id", "*80000001"),
            ("dst-address", "0.0.0.0/0"),
            ("routing-table", "main"),
            ("gateway", "10.0.2.2"),
            ("immediate-gw", "10.0.2.2%ether1"),
            ("distance", "1"),
            ("dynamic", "true"),
            ("inactive", "false"),
            ("active", "true"),
            ("dhcp", "true"),
        ],
    )
}

fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(name, value)| (Box::from(*name), Some(Box::from(*value))))
        .collect()
}

#[test]
fn the_gateway_is_the_active_default_route_s_next_hop() {
    let connected = parsed::<IpRoute>(&[
        (".id", "*20183010"),
        ("dst-address", "10.0.2.0/24"),
        ("gateway", "ether1"),
        ("immediate-gw", "ether1"),
        ("distance", "0"),
        ("dynamic", "true"),
        ("inactive", "false"),
        ("active", "true"),
    ]);
    let standby = route(&[
        ("gateway", "192.0.2.1"),
        ("active", "false"),
        ("inactive", "true"),
    ]);
    let other_table = route(&[("gateway", "192.0.2.2"), ("routing-table", "vpn")]);
    let routes = [connected, standby, other_table, route(&[])];
    assert_eq!(default_gateway(&routes), Some([10, 0, 2, 2].into()));
    assert_eq!(default_gateway(&routes[..3]), None);
}

/// A recursive route: its gateway is far away, the next hop is the uplink's.
#[test]
fn a_recursive_route_counts_its_next_hop() {
    let recursive = route(&[
        ("gateway", "198.51.100.1"),
        ("immediate-gw", "192.0.2.1%ether1"),
    ]);
    assert_eq!(default_gateway(&[recursive]), Some([192, 0, 2, 1].into()));
}

#[test]
fn the_last_row_counts_the_pings() {
    let gateway = [10, 0, 2, 2].into();
    let rows = [
        row(&[("seq", "0"), ("sent", "1"), ("received", "1")]),
        row(&[
            ("seq", "4"),
            ("sent", "5"),
            ("received", "4"),
            ("packet-loss", "20"),
            ("avg-rtt", "1ms234us"),
        ]),
    ];
    let ping = summary(gateway, &rows);
    assert_eq!(
        ping,
        UplinkPing {
            gateway,
            sent: 5,
            received: 4,
            rtt: Some(Duration::from_micros(1234)),
        }
    );
    assert_eq!(ping.loss(), 0.2);
    let lost = summary(gateway, &[row(&[("sent", "5"), ("received", "0")])]);
    assert_eq!((lost.rtt, lost.loss()), (None, 1.0));
    assert_eq!(summary(gateway, &[]).loss(), 1.0);
}

#[test]
fn a_round_trip_in_routeros_units() {
    assert_eq!(rtt("342us"), Some(Duration::from_micros(342)));
    assert_eq!(rtt("12ms"), Some(Duration::from_millis(12)));
    assert_eq!(rtt("1s5ms"), Some(Duration::from_millis(1005)));
    assert_eq!(rtt("12"), None);
}
