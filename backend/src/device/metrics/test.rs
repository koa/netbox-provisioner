//! Scope: Tests of `device::metrics`'s parsing; nothing else.

use super::{
    Capsman, bits_per_second, ethernet_link, interface_counters, standalone_client, wireless_radio,
};
use crate::device::raw::{Row, is_missing_menu};
use mikrotik_model::mikrotik_api::TrapCategory;
use std::{collections::HashMap, time::Duration};

fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(name, value)| (Box::from(*name), Some(Box::from(*value))))
        .collect()
}

/// lab-crs125's ether1 (7.24.6): the counters without `stats`, no error or drop counters.
#[test]
fn interface_counters_of_a_row() {
    let counters = interface_counters(&row(&[
        ("name", "e01-lab-np2"),
        ("type", "ether"),
        ("running", "true"),
        ("rx-byte", "1531142"),
        ("tx-byte", "3074323"),
        ("rx-packet", "12362"),
        ("tx-packet", "11994"),
        ("link-downs", "0"),
    ]))
    .unwrap();
    assert_eq!(&*counters.name, "e01-lab-np2");
    assert!(counters.running);
    assert_eq!(counters.tx_bytes, Some(3074323));
    assert_eq!(counters.rx_errors, None);
    assert_eq!(counters.link_downs, Some(0));
    assert!(interface_counters(&row(&[("type", "ether")])).is_none());
}

/// A missing package's menu has no rows; another refusal stays an error.
#[test]
fn a_missing_menu_is_told_apart() {
    assert!(is_missing_menu(None, b"no such command prefix"));
    assert!(is_missing_menu(
        Some(TrapCategory::MissingItemOrCommand),
        b""
    ));
    assert!(!is_missing_menu(None, b"not enough permissions (9)"));
}

/// lab-sxt-a's station on `w01` (7.24.6): bytes and packets `rx,tx`, the SSID its radio's.
#[test]
fn a_client_of_a_standalone_radio() {
    let ssids = HashMap::from([("w01", "lab-link-2")]);
    let client = standalone_client(
        &row(&[
            ("interface", "w01"),
            ("mac-address", "E4:8D:8C:76:C5:0E"),
            ("bytes", "98366,162610"),
            ("packets", "270,865"),
            ("signal-strength", "-71"),
            ("signal-to-noise", "36"),
            ("uptime", "4d6h54m49s"),
        ]),
        &ssids,
    )
    .unwrap();
    assert_eq!(client.mac_address.to_string(), "E4:8D:8C:76:C5:0E");
    assert_eq!(&*client.ssid, "lab-link-2");
    assert_eq!(client.capsman, Capsman::Standalone);
    assert_eq!((client.rx_bytes, client.tx_bytes), (98366, 162610));
    assert_eq!((client.rx_packets, client.tx_packets), (270, 865));
    assert_eq!(client.signal, -71);
    assert_eq!(client.signal_to_noise, Some(36));
    assert_eq!(
        client.uptime,
        Duration::from_secs(((4 * 24 + 6) * 60 + 54) * 60 + 49)
    );
    let older = standalone_client(
        &row(&[
            ("interface", "w01"),
            ("mac-address", "E4:8D:8C:76:C5:0E"),
            ("bytes", "1,2"),
            ("packets", "3,4"),
            ("signal-strength", "-71@6Mbps"),
            ("uptime", "1s"),
        ]),
        &ssids,
    )
    .unwrap();
    assert_eq!(older.signal, -71);
    assert_eq!(older.signal_to_noise, None);
}

/// lab-sxt-a's `w01`: the frequency from `channel`, the noise floor from `monitor`.
#[test]
fn a_running_wireless_radio() {
    let radio = wireless_radio(
        &row(&[
            ("name", "w01"),
            ("ssid", "lab-link-2"),
            ("band", "5ghz-a/n/ac"),
        ]),
        &row(&[
            ("channel", "5500/20-Ceee/ac/DP(13dBm)"),
            ("noise-floor", "-106"),
        ]),
    )
    .unwrap();
    assert_eq!(radio.frequency_mhz, Some(5500));
    assert_eq!(radio.noise_floor, Some(-106));
    assert_eq!(&*radio.band, "5ghz-a/n/ac");
}

/// lab-crs125's ports: a link with its rate, one without; the rate only while it is up.
#[test]
fn ethernet_links() {
    let up = ethernet_link(&row(&[
        ("name", "e01-lab-np2"),
        ("status", "link-ok"),
        ("rate", "1Gbps"),
    ]))
    .unwrap();
    assert!(up.up);
    assert_eq!(up.rate_bits_per_second, Some(1_000_000_000));
    let down = ethernet_link(&row(&[("name", "e02"), ("status", "no-link")])).unwrap();
    assert!(!down.up);
    assert_eq!(down.rate_bits_per_second, None);
    assert_eq!(bits_per_second("2.5Gbps"), Some(2_500_000_000));
    assert_eq!(bits_per_second("100Mbps"), Some(100_000_000));
    assert_eq!(bits_per_second("10Kbps"), Some(10_000));
    assert_eq!(bits_per_second("auto"), None);
}
