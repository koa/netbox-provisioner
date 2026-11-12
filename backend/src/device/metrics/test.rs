//! Scope: Tests of `device::metrics`'s parsing; nothing else.

use super::{
    Capsman, LegacyCapsman, bits_per_second, capsman_radio_rows, ethernet_link, interface_counters,
    is_missing_menu, standalone_client, wireless_radio,
};
use crate::device::raw::Row;
use mikrotik_model::{
    ascii::AsciiString,
    mikrotik_api::TrapCategory,
    resource::{AppendFieldResult, DeserializeRosBuilder, DeserializeRosResource},
};
use std::{collections::HashMap, time::Duration};

fn row(fields: &[(&str, &str)]) -> Row {
    fields
        .iter()
        .map(|(name, value)| (Box::from(*name), Some(Box::from(*value))))
        .collect()
}

/// `fields` as the model `R` reads them from the device.
fn parse<R: DeserializeRosResource>(fields: &[(&str, &str)]) -> R
where
    <R::Builder as DeserializeRosBuilder<R>>::Context: Default,
{
    let mut builder = R::Builder::init(&Default::default());
    for (name, value) in fields {
        if let AppendFieldResult::InvalidValue(field) =
            builder.append_field(name.as_bytes(), Some(value.as_bytes()))
        {
            panic!("{name}={value}: {}", String::from_utf8_lossy(field));
        }
    }
    builder
        .build()
        .unwrap_or_else(|missing| panic!("missing {}", String::from_utf8_lossy(missing)))
}

fn ascii(value: &str) -> AsciiString {
    value.into()
}

/// lab-crs125's ether1 (7.24.6): the counters without `stats`, no error or drop counters.
#[test]
fn interface_counters_of_an_interface() {
    let counters = interface_counters(&parse(&[
        (".id", "*1"),
        ("name", "e01-lab-np2"),
        ("type", "ether"),
        ("running", "true"),
        ("rx-byte", "1531142"),
        ("tx-byte", "3074323"),
        ("rx-packet", "12362"),
        ("tx-packet", "11994"),
        ("link-downs", "0"),
    ]));
    assert_eq!(&*counters.name, "e01-lab-np2");
    assert!(counters.running);
    assert_eq!(counters.tx_bytes, Some(3074323));
    assert_eq!(counters.rx_errors, None);
    assert_eq!(counters.link_downs, Some(0));
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
    let (w01, link) = (ascii("w01"), ascii("lab-link-2"));
    let ssids = HashMap::from([(&w01, &link)]);
    let client = standalone_client(
        &parse(&[
            (".id", "*3"),
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
        &parse(&[
            (".id", "*3"),
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
        &parse(&wireless_fields(&[
            ("name", "w01"),
            ("ssid", "lab-link-2"),
            ("band", "5ghz-a/n/ac"),
        ])),
        &row(&[
            ("channel", "5500/20-Ceee/ac/DP(13dBm)"),
            ("noise-floor", "-106"),
        ]),
    );
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

/// On a CAP of the legacy CAPsMAN (ap-klr-solar) a radio has no SSID of its own: it is the
/// CAPsMAN's, and so are its clients.
#[test]
fn a_radio_the_capsman_manages() {
    let radio = wireless_radio(
        &parse(&wireless_fields(&[
            ("name", "wlan13"),
            ("ssid", ""),
            ("running", "true"),
        ])),
        &row(&[("channel", "2412/20/gn"), ("noise-floor", "-110")]),
    );
    assert_eq!(radio.capsman, Capsman::Legacy);
    assert_eq!(&*radio.ssid, "");
    assert_eq!(radio.frequency_mhz, Some(2412));
    let (w01, link) = (ascii("w01"), ascii("lab-link-2"));
    let ssids = HashMap::from([(&w01, &link)]);
    let client = parse(&[
        (".id", "*4"),
        ("interface", "wlan13"),
        ("mac-address", "E4:8D:8C:76:C5:0E"),
        ("bytes", "1,2"),
        ("packets", "3,4"),
        ("signal-strength", "-60"),
        ("uptime", "1s"),
    ]);
    assert!(standalone_client(&client, &ssids).is_none());
}

/// The controller's view of a legacy CAP's radios: SSID and band of the actual configuration,
/// a virtual interface on its master's CAP and band, only running ones.
#[test]
fn the_radios_of_the_legacy_caps() {
    let interfaces = [
        capsman_interface(&[
            ("name", "cap-solar-1"),
            ("running", "true"),
            ("current-channel", "2437/20-Ce/gn(20dBm)"),
            ("master-interface", "none"),
        ]),
        capsman_interface(&[
            ("name", "cap-solar-gast"),
            ("running", "true"),
            ("current-channel", "2437/20-Ce/gn(20dBm)"),
            ("master-interface", "cap-solar-1"),
        ]),
        capsman_interface(&[
            ("name", "cap-solar-2"),
            ("running", "false"),
            ("master-interface", "none"),
        ]),
    ];
    let actual = [
        actual_configuration(&[
            ("name", "cap-solar-1"),
            ("configuration.ssid", "berg"),
            ("channel.band", "2ghz-b/g/n"),
        ]),
        actual_configuration(&[("name", "cap-solar-gast"), ("configuration.ssid", "gast")]),
    ];
    let capsman = LegacyCapsman {
        interfaces: Box::new(interfaces),
        radios: Box::new([parse(&[
            (".id", "*1"),
            ("interface", "cap-solar-1"),
            ("remote-cap-identity", "ap-klr-solar"),
            ("remote-cap-name", "[02:00:00:00:00:10]"),
            ("radio-mac", "02:00:00:00:00:10"),
            ("local", "false"),
            ("provisioned", "true"),
        ])]),
    };
    let found = capsman_radio_rows(&capsman, &actual);
    let summary = found
        .iter()
        .map(|radio| {
            format!(
                "{} {} {} {} {:?}",
                radio.name, radio.ssid, radio.ap, radio.band, radio.frequency_mhz
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            "cap-solar-1 berg ap-klr-solar 2ghz-b/g/n Some(2437)",
            "cap-solar-gast gast ap-klr-solar 2ghz-b/g/n Some(2437)",
        ]
    );
    assert!(found.iter().all(|radio| radio.capsman == Capsman::Legacy));
}

/// lab-sxt-a's `w01` as `/interface/wireless/print` gives it (7.24.6), the keys replaced.
const SXT_A_W01: &[(&str, &str)] = &[
    (".id", "*3"),
    ("adaptive-noise-immunity", "none"),
    ("allow-sharedkey", "false"),
    ("ampdu-priorities", "0"),
    ("amsdu-limit", "8192"),
    ("amsdu-threshold", "8192"),
    ("antenna-gain", "14"),
    ("area", ""),
    ("arp", "enabled"),
    ("arp-timeout", "auto"),
    ("band", "5ghz-onlyac"),
    ("basic-rates-a/g", "6Mbps"),
    ("bridge-mode", "enabled"),
    ("channel-width", "20/40/80mhz-XXXX"),
    ("compression", "false"),
    ("country", "switzerland"),
    ("default-ap-tx-limit", "0"),
    ("default-authentication", "true"),
    ("default-client-tx-limit", "0"),
    ("default-forwarding", "true"),
    ("default-name", "wlan1"),
    ("disable-running-check", "false"),
    ("disabled", "false"),
    ("disconnect-timeout", "3s"),
    ("distance", "dynamic"),
    ("frame-lifetime", "0"),
    ("frequency", "auto"),
    ("frequency-mode", "regulatory-domain"),
    ("frequency-offset", "0"),
    ("guard-interval", "any"),
    ("hide-ssid", "false"),
    (
        "ht-basic-mcs",
        "mcs-0,mcs-1,mcs-2,mcs-3,mcs-4,mcs-5,mcs-6,mcs-7",
    ),
    (
        "ht-supported-mcs",
        "mcs-0,mcs-1,mcs-2,mcs-3,mcs-4,mcs-5,mcs-6,mcs-7,mcs-8,mcs-9,mcs-10,mcs-11,mcs-12,mcs-13,mcs-14,mcs-15,mcs-16,mcs-17,mcs-18,mcs-19,mcs-20,mcs-21,mcs-22,mcs-23",
    ),
    ("hw-fragmentation-threshold", "disabled"),
    ("hw-protection-mode", "none"),
    ("hw-protection-threshold", "0"),
    ("hw-retries", "7"),
    ("installation", "outdoor"),
    ("interface-type", "Atheros AR9888"),
    ("interworking-profile", "disabled"),
    ("keepalive-frames", "enabled"),
    ("l2mtu", "2004"),
    ("mac-address", "4C:5E:0C:19:33:09"),
    ("max-station-count", "2007"),
    ("mode", "bridge"),
    ("mtu", "1500"),
    ("multicast-buffering", "enabled"),
    ("multicast-helper", "default"),
    ("name", "w01"),
    ("nv2-cell-radius", "30"),
    ("nv2-downlink-ratio", "50"),
    ("nv2-mode", "dynamic-downlink"),
    ("nv2-preshared-key", "not-the-key"),
    ("nv2-qos", "default"),
    ("nv2-queue-count", "2"),
    ("nv2-security", "enabled"),
    ("nv2-sync-secret", "not-the-key"),
    ("on-fail-retry-time", "100ms"),
    ("pci-info", "00:00.0"),
    ("preamble-mode", "both"),
    ("radio-name", "4C5E0C193309"),
    ("rate-selection", "advanced"),
    ("rate-set", "default"),
    ("running", "true"),
    ("rx-chains", "0,1"),
    ("scan-list", "default"),
    ("secondary-frequency", ""),
    ("security-profile", "default"),
    ("skip-dfs-channels", "disabled"),
    ("ssid", "lab-link-2"),
    ("station-bridge-clone-mac", "00:00:00:00:00:00"),
    ("station-roaming", "disabled"),
    (
        "supported-rates-a/g",
        "6Mbps,9Mbps,12Mbps,18Mbps,24Mbps,36Mbps,48Mbps,54Mbps",
    ),
    ("tdma-period-size", "auto"),
    ("tx-chains", "0,1"),
    ("tx-power", "5"),
    ("tx-power-mode", "all-rates-fixed"),
    ("update-stats-interval", "disabled"),
    ("vht-basic-mcs", "mcs0-7"),
    ("vht-supported-mcs", "mcs0-9,mcs0-9,mcs0-9"),
    ("vlan-id", "1"),
    ("vlan-mode", "no-tag"),
    ("wds-cost-range", "50-150"),
    ("wds-default-bridge", "none"),
    ("wds-default-cost", "100"),
    ("wds-ignore-ssid", "false"),
    ("wds-mode", "disabled"),
    ("wireless-protocol", "nv2"),
    ("wmm-support", "disabled"),
    ("wps-mode", "disabled"),
];

/// `fields` over [`SXT_A_W01`].
fn wireless_fields<'f>(fields: &[(&'f str, &'f str)]) -> Vec<(&'f str, &'f str)> {
    with_fields(SXT_A_W01, fields)
}

/// `base` with `fields` set over it.
fn with_fields<'f>(
    base: &[(&'f str, &'f str)],
    fields: &[(&'f str, &'f str)],
) -> Vec<(&'f str, &'f str)> {
    let mut all = base
        .iter()
        .filter(|(name, _)| fields.iter().all(|(set, _)| set != name))
        .copied()
        .collect::<Vec<_>>();
    all.extend_from_slice(fields);
    all
}

/// The fields RouterOS gives every row of `/caps-man/interface`, made up but for the names.
const CAPSMAN_INTERFACE: &[(&str, &str)] = &[
    (".id", "*1"),
    ("mac-address", "02:00:00:00:00:10"),
    ("radio-mac", "02:00:00:00:00:10"),
    ("current-rate-set", ""),
    ("current-basic-rate-set", ""),
    ("current-state", "running-ap"),
    ("current-registered-clients", "0"),
    ("current-authorized-clients", "0"),
    ("master", "true"),
    ("bound", "true"),
    ("dynamic", "false"),
    ("inactive", "false"),
];

fn capsman_interface(fields: &[(&str, &str)]) -> mikrotik_model::model::CapsManInterface {
    parse(&with_fields(CAPSMAN_INTERFACE, fields))
}

/// The fields RouterOS gives every row of `/caps-man/actual-interface-configuration`, made up
/// but the country, which the legacy CAPsMAN writes in lowercase.
const ACTUAL_CONFIGURATION: &[(&str, &str)] = &[
    (".id", "*1"),
    ("configuration.country", "switzerland"),
    ("arp-timeout", "auto"),
    ("disabled", "false"),
    ("l2mtu", "1600"),
    ("mac-address", "02:00:00:00:00:10"),
    ("radio-mac", "02:00:00:00:00:10"),
    ("inactive", "false"),
    ("master", "true"),
    ("bound", "true"),
    ("dynamic", "false"),
    ("running", "true"),
];

fn actual_configuration(
    fields: &[(&str, &str)],
) -> mikrotik_model::model::CapsManActualInterfaceConfiguration {
    parse(&with_fields(ACTUAL_CONFIGURATION, fields))
}
