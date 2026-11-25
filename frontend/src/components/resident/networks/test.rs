//! Scope: Tests of how a network's internet is judged; nothing else.
use super::{Internet, judge};
use crate::graphql::resident::{
    live::{NetworkInternet, UplinkQuality, WayHop},
    residents::Named,
};

fn hop(id: i32, name: &str, reached: Option<bool>) -> WayHop {
    WayHop {
        device: Named {
            id,
            name: name.into(),
        },
        reached,
    }
}

fn internet(way: Vec<WayHop>, uplink: Option<UplinkQuality>) -> NetworkInternet {
    NetworkInternet {
        network: 1,
        way,
        has_uplink: true,
        uplink,
    }
}

fn pings(loss: f64) -> Option<UplinkQuality> {
    Some(UplinkQuality {
        loss,
        rtt_ms: Some(12.0),
    })
}

#[test]
fn a_router_not_reached_cuts_the_way() {
    let way = vec![
        hop(1, "haus", Some(true)),
        hop(2, "schopf", Some(false)),
        hop(3, "core", Some(true)),
    ];
    assert_eq!(
        judge(&internet(way, pings(0.0))),
        Internet::Cut("schopf".into())
    );
}

#[test]
fn the_uplink_s_pings_decide_otherwise() {
    let way = || vec![hop(1, "haus", Some(true)), hop(3, "core", None)];
    assert_eq!(
        judge(&internet(way(), pings(0.0))),
        Internet::Measured {
            loss: 0.0,
            rtt_ms: Some(12.0)
        }
    );
    assert_eq!(judge(&internet(way(), pings(1.0))), Internet::NoAnswer);
    assert_eq!(judge(&internet(way(), None)), Internet::Unmeasured);
}
