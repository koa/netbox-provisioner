//! Scope: Tests of the rendering of `/metrics`; nothing else.

use super::render::{DeviceSample, Snapshot, WlanGroupSample, families, wlan_group_of};
use super::{DEVICE_TIMEOUT, device_budget};
use crate::{
    device::metrics::{
        Capsman, DeviceMetrics, EthernetLink, InterfaceCounters, Lease, WirelessRadio, WlanClient,
    },
    device::poe::{PoePorts, monitor_reading},
    network_state::device_data::{DeviceMapData, HealthRow, Resources},
};
use mikrotik_model::MacAddress;
use prometheus::proto::MetricFamily;
use std::time::Duration;

fn client() -> MacAddress {
    MacAddress::new([0x02, 0, 0, 0, 0, 0x01])
}

fn group(name: &str, ssids: &[&str], aps: &[&str]) -> WlanGroupSample {
    WlanGroupSample {
        name: name.into(),
        ssids: ssids.iter().map(|ssid| Box::from(*ssid)).collect(),
        aps: aps.iter().map(|ap| Box::from(*ap)).collect(),
    }
}

/// The router with the lease of the client, the AP the client is on, a device that didn't
/// answer.
fn snapshot() -> Snapshot {
    let router = DeviceSample {
        device: "router".into(),
        site: "berg".into(),
        up: true,
        map: Some(DeviceMapData {
            health: vec![
                HealthRow {
                    name: "temperature".into(),
                    value: "41".into(),
                    unit: Some("C".into()),
                },
                HealthRow {
                    name: "psu1-state".into(),
                    value: "ok".into(),
                    unit: None,
                },
            ]
            .into(),
            resources: Some(Resources {
                uptime: Duration::from_secs(93784),
                cpu_load: 12,
                free_memory: 1000,
                total_memory: 4000,
                free_hdd_space: 500,
                total_hdd_space: 16000,
                version: "7.24.6 (long-term)".into(),
                board_name: "hAP ax^2".into(),
            }),
            poe: PoePorts {
                ethernet: Box::default(),
                monitor: vec![monitor_reading(&[
                    ("name", "e07-lab-crs125"),
                    ("poe-out-status", "powered-on"),
                    ("poe-out-voltage", "24.1"),
                    ("poe-out-current", "150"),
                    ("poe-out-power", "3.6"),
                ])]
                .into(),
            },
            poe_load: Box::default(),
        }),
        metrics: Some(DeviceMetrics {
            wlan_clients: Box::new([]),
            radios: Box::new([]),
            ethernet: Box::new([
                EthernetLink {
                    name: "e01-uplink".into(),
                    up: true,
                    rate_bits_per_second: Some(1_000_000_000),
                },
                EthernetLink {
                    name: "e02".into(),
                    up: false,
                    rate_bits_per_second: None,
                },
            ]),
            interfaces: Box::new([InterfaceCounters {
                name: "e01-uplink".into(),
                kind: "ether".into(),
                running: true,
                rx_bytes: Some(1531142),
                tx_bytes: Some(3074323),
                rx_packets: Some(12362),
                tx_packets: Some(11994),
                rx_errors: None,
                tx_errors: None,
                rx_drops: None,
                tx_drops: None,
                link_downs: Some(0),
            }]),
            leases: Box::new([Lease {
                server: "dhcp-clients".into(),
                address: "10.0.1.20".parse().unwrap(),
                mac_address: client(),
                hostname: Some("laptop".into()),
            }]),
        }),
        wlan_groups: Box::new([]),
    };
    let ap = DeviceSample {
        device: "ap".into(),
        site: "berg".into(),
        up: true,
        map: None,
        metrics: Some(DeviceMetrics {
            wlan_clients: Box::new([WlanClient {
                mac_address: client(),
                interface: "cap-stall-1".into(),
                ssid: "berg".into(),
                ap: "cap-stall".into(),
                capsman: Capsman::Wifi,
                rx_bytes: 100,
                tx_bytes: 200,
                rx_packets: 3,
                tx_packets: 4,
                signal: -61,
                signal_to_noise: None,
                uptime: Duration::from_secs(90),
            }]),
            radios: Box::new([
                WirelessRadio {
                    name: "w01".into(),
                    ssid: "lab-link-2".into(),
                    ap: "".into(),
                    capsman: Capsman::Standalone,
                    band: "5ghz-a/n/ac".into(),
                    frequency_mhz: Some(5500),
                    noise_floor: Some(-106),
                },
                WirelessRadio {
                    name: "wlan13".into(),
                    ssid: "".into(),
                    ap: "".into(),
                    capsman: Capsman::Legacy,
                    band: "".into(),
                    frequency_mhz: Some(2412),
                    noise_floor: Some(-110),
                },
                WirelessRadio {
                    name: "cap-stall-1".into(),
                    ssid: "berg".into(),
                    ap: "cap-stall".into(),
                    capsman: Capsman::Legacy,
                    band: "2ghz-b/g/n".into(),
                    frequency_mhz: Some(2437),
                    noise_floor: None,
                },
            ]),
            ethernet: Box::new([]),
            interfaces: Box::new([]),
            leases: Box::new([]),
        }),
        wlan_groups: Box::new([group("wlan-berg", &["berg"], &["cap-stall"])]),
    };
    let down = DeviceSample {
        device: "gone".into(),
        site: "tal".into(),
        up: false,
        map: None,
        metrics: None,
        wlan_groups: Box::new([]),
    };
    Snapshot {
        devices: Box::new([router, ap, down]),
    }
}

/// `name`'s samples as their labels (`name=value`, sorted) and value, sorted.
fn samples(families: &[MetricFamily], name: &str) -> Vec<(Vec<String>, f64)> {
    let Some(family) = families.iter().find(|family| family.name() == name) else {
        return Vec::new();
    };
    let mut samples = family
        .get_metric()
        .iter()
        .map(|metric| {
            let mut labels: Vec<String> = metric
                .get_label()
                .iter()
                .map(|label| format!("{}={}", label.name(), label.value()))
                .collect();
            labels.sort();
            let value = match family.get_field_type() {
                prometheus::proto::MetricType::COUNTER => metric.get_counter().value(),
                _ => metric.get_gauge().value(),
            };
            (labels, value)
        })
        .collect::<Vec<_>>();
    samples.sort_by(|a, b| a.0.cmp(&b.0));
    samples
}

fn labels(values: &[&str]) -> Vec<String> {
    let mut labels = values.iter().map(ToString::to_string).collect::<Vec<_>>();
    labels.sort();
    labels
}

/// Every device has `mikrotik_up`; one that didn't answer has nothing else.
#[test]
fn every_device_is_up_or_down() {
    let families = families(&snapshot());
    assert_eq!(
        samples(&families, "mikrotik_up"),
        [
            (labels(&["device=ap", "site=berg"]), 1.0),
            (labels(&["device=gone", "site=tal"]), 0.0),
            (labels(&["device=router", "site=berg"]), 1.0),
        ]
    );
    assert!(
        families
            .iter()
            .flat_map(|family| family.get_metric())
            .filter(|metric| metric
                .get_label()
                .iter()
                .any(|label| label.value() == "gone"))
            .count()
            == 1
    );
}

/// A WLAN client takes hostname and address from the lease of another device, by MAC; its
/// byte counts are counters.
#[test]
fn a_wlan_client_is_named_by_its_lease() {
    let families = families(&snapshot());
    let client = labels(&[
        "device=ap",
        "site=berg",
        "interface=cap-stall-1",
        "wlan_group=wlan-berg",
        "ap=cap-stall",
        "ssid=berg",
        "capsman=wifi",
        "mac_address=02:00:00:00:00:01",
        "client_hostname=laptop",
        "client_ip=10.0.1.20",
    ]);
    assert_eq!(
        samples(&families, "mikrotik_wlan_client_rx_bytes_total"),
        [(client.clone(), 100.0)]
    );
    assert_eq!(
        samples(&families, "mikrotik_wlan_client_signal_dbm"),
        [(client, -61.0)]
    );
    assert!(samples(&families, "mikrotik_wlan_client_signal_to_noise_db").is_empty());
    let family = families
        .iter()
        .find(|family| family.name() == "mikrotik_wlan_client_rx_bytes_total")
        .unwrap();
    assert_eq!(
        family.get_field_type(),
        prometheus::proto::MetricType::COUNTER
    );
}

/// Health by unit (a state without one left out), resources with uptime and load as numbers,
/// PoE current from mA, interface counters only where the device reports them.
#[test]
fn health_resources_poe_and_interfaces() {
    let families = families(&snapshot());
    let router = labels(&["device=router", "site=berg"]);
    assert_eq!(
        samples(&families, "mikrotik_health_temperature_celsius"),
        [(
            labels(&["device=router", "site=berg", "name=temperature"]),
            41.0
        )]
    );
    assert!(samples(&families, "mikrotik_health_voltage_volts").is_empty());
    assert_eq!(
        samples(&families, "mikrotik_system_uptime_seconds"),
        [(router.clone(), 93784.0)]
    );
    assert_eq!(
        samples(&families, "mikrotik_system_cpu_load_ratio"),
        [(router.clone(), 0.12)]
    );
    assert_eq!(
        samples(&families, "mikrotik_system_info"),
        [(
            labels(&[
                "device=router",
                "site=berg",
                "version=7.24.6 (long-term)",
                "board=hAP ax^2"
            ]),
            1.0
        )]
    );
    let port = labels(&["device=router", "site=berg", "interface=e07-lab-crs125"]);
    assert_eq!(
        samples(&families, "mikrotik_poe_out_current_amperes"),
        [(port.clone(), 0.15)]
    );
    assert_eq!(
        samples(&families, "mikrotik_poe_out_power_watts"),
        [(port, 3.6)]
    );
    let uplink = labels(&["device=router", "site=berg", "interface=e01-uplink"]);
    assert_eq!(
        samples(&families, "mikrotik_interface_tx_bytes_total"),
        [(uplink.clone(), 3074323.0)]
    );
    assert!(samples(&families, "mikrotik_interface_rx_errors_total").is_empty());
    assert_eq!(
        samples(&families, "mikrotik_ethernet_link_up"),
        [
            (uplink.clone(), 1.0),
            (
                labels(&["device=router", "site=berg", "interface=e02"]),
                0.0
            ),
        ]
    );
    assert_eq!(
        samples(&families, "mikrotik_ethernet_rate_bits_per_second"),
        [(uplink.clone(), 1e9)]
    );
    assert_eq!(
        samples(&families, "mikrotik_dhcp_lease"),
        [(
            labels(&[
                "device=router",
                "site=berg",
                "server=dhcp-clients",
                "address=10.0.1.20",
                "mac_address=02:00:00:00:00:01",
                "hostname=laptop",
            ]),
            1.0
        )]
    );
}

/// A running radio: SSID and band as labels, the frequency in Hz; a CAP's radio its CAPsMAN
/// manages (no SSID) has no `info`, the controller's has its CAP.
#[test]
fn a_wireless_radio() {
    let families = families(&snapshot());
    assert_eq!(
        samples(&families, "mikrotik_wireless_radio_info"),
        [
            (
                labels(&[
                    "device=ap",
                    "site=berg",
                    "interface=w01",
                    "ssid=lab-link-2",
                    "band=5ghz-a/n/ac",
                    "wlan_group=",
                    "ap=",
                    "capsman=none",
                ]),
                1.0
            ),
            (
                labels(&[
                    "device=ap",
                    "site=berg",
                    "interface=cap-stall-1",
                    "ssid=berg",
                    "band=2ghz-b/g/n",
                    "wlan_group=wlan-berg",
                    "ap=cap-stall",
                    "capsman=legacy",
                ]),
                1.0
            )
        ]
    );
    let radio = labels(&["device=ap", "site=berg", "interface=w01"]);
    assert_eq!(
        samples(&families, "mikrotik_wireless_radio_frequency_hertz"),
        [
            (
                labels(&["device=ap", "site=berg", "interface=cap-stall-1"]),
                2.437e9
            ),
            (radio.clone(), 5.5e9),
            (
                labels(&["device=ap", "site=berg", "interface=wlan13"]),
                2.412e9
            ),
        ]
    );
    assert_eq!(
        samples(&families, "mikrotik_wireless_radio_noise_floor_dbm"),
        [
            (radio, -106.0),
            (
                labels(&["device=ap", "site=berg", "interface=wlan13"]),
                -110.0
            ),
        ]
    );
}

/// The group of a client or radio: the one with its SSID and, for a CAP's client, that CAP;
/// none where several or none fit.
#[test]
fn the_wlan_group_by_ssid_and_ap() {
    let groups = [
        group("wlan-berg", &["berg", "gast"], &["cap-stall"]),
        group("wlan-tal", &["tal", "gast"], &["cap-hof"]),
    ];
    assert_eq!(wlan_group_of(&groups, "berg", ""), "wlan-berg");
    assert_eq!(wlan_group_of(&groups, "gast", "cap-hof"), "wlan-tal");
    assert_eq!(wlan_group_of(&groups, "gast", ""), "");
    assert_eq!(wlan_group_of(&groups, "berg", "cap-hof"), "");
    assert_eq!(wlan_group_of(&groups, "lab-link-2", ""), "");
}

/// A scrape's families go through the text encoder, which refuses a family without a value:
/// the sample's devices have no fan.
#[test]
fn the_families_encode_without_the_empty_ones() {
    let families = families(&snapshot());
    assert!(
        families
            .iter()
            .all(|family| !family.get_metric().is_empty())
    );
    let mut text = Vec::new();
    prometheus::Encoder::encode(&prometheus::TextEncoder::new(), &families, &mut text).unwrap();
    assert!(!String::from_utf8_lossy(&text).contains("mikrotik_health_fan_rpm"));
}

/// A scrape answers before Prometheus gives up on it: Prometheus' default 10 s leave the devices
/// 9 s, the chart's 25 s at most `DEVICE_TIMEOUT`, as does a scrape without the header.
#[test]
fn the_devices_have_the_time_prometheus_waits() {
    assert_eq!(
        device_budget(Some(Duration::from_secs(10))),
        Duration::from_secs(9)
    );
    assert_eq!(device_budget(Some(Duration::from_secs(25))), DEVICE_TIMEOUT);
    assert_eq!(device_budget(None), DEVICE_TIMEOUT);
    assert_eq!(
        device_budget(Some(Duration::from_millis(500))),
        Duration::ZERO
    );
}
