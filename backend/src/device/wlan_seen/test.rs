//! Scope: Tests of `device::wlan_seen`; nothing else.

use super::{Seen, SeenClient};
use crate::{
    device::metrics::{Capsman, WlanClient},
    topology::DeviceId,
};
use std::time::Duration;
use tokio::time::Instant;

fn client(mac: &str, ap: &str) -> WlanClient {
    WlanClient {
        mac_address: mac.parse().unwrap(),
        interface: "cap-1".into(),
        ssid: "berg".into(),
        ap: ap.into(),
        capsman: Capsman::Wifi,
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
        signal: -60,
        signal_to_noise: None,
        uptime: Duration::from_secs(60),
        band: None,
    }
}

fn macs(seen: &Seen, ap: &str) -> Vec<String> {
    seen.of(DeviceId(1), ap)
        .map(|seen| {
            seen.clients
                .iter()
                .map(|c| c.mac_address.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// An access point gone from the controller keeps the clients of the read it was last in,
/// with that read's time; one in the read gets its new clients; the controller's own radios
/// count under its name; a day later the old entry goes.
#[test]
fn an_access_point_gone_keeps_its_last_clients() {
    let controller = DeviceId(1);
    let mut seen = Seen::default();
    let first = Instant::now();
    seen.note(
        controller,
        "core",
        &[
            client("02:00:00:00:00:01", "cap-a"),
            client("02:00:00:00:00:02", "cap-b"),
            client("02:00:00:00:00:03", ""),
        ],
        first,
    );
    let later = first + Duration::from_secs(60);
    seen.note(
        controller,
        "core",
        &[client("02:00:00:00:00:04", "cap-b")],
        later,
    );
    assert_eq!(macs(&seen, "cap-a"), ["02:00:00:00:00:01"]);
    assert_eq!(seen.of(controller, "cap-a").unwrap().at, first);
    assert_eq!(macs(&seen, "cap-b"), ["02:00:00:00:00:04"]);
    assert_eq!(seen.of(controller, "cap-b").unwrap().at, later);
    assert_eq!(macs(&seen, "core"), ["02:00:00:00:00:03"]);
    assert_eq!(
        seen.of(controller, "cap-a").unwrap().clients[0],
        SeenClient {
            mac_address: "02:00:00:00:00:01".parse().unwrap(),
            ssid: "berg".into(),
        }
    );
    seen.note(
        controller,
        "core",
        &[],
        first + Duration::from_secs(24 * 3600),
    );
    assert_eq!(macs(&seen, "cap-a"), Vec::<String>::new());
    assert_eq!(macs(&seen, "cap-b"), ["02:00:00:00:00:04"]);
}
