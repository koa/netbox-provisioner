use mikrotik_model::{
    MacAddress,
    ascii::AsciiString,
    hwconfig::{
        ADVERTISE_1G, ADVERTISE_1G_FULL, ADVERTISE_1G_SFP, ADVERTISE_10G, ADVERTISE_10G_FULL,
        ADVERTISE_100M, EthernetNamePattern, generate_ethernet, generate_wifi, generate_wlan,
    },
    model::{
        EnabledDisabled, InterfaceEthernetByDefaultName, InterfaceW60GArp,
        InterfaceW60GByDefaultName, InterfaceW60GCfg, InterfaceWifiByDefaultName,
        InterfaceWirelessAdaptiveNoiseImmunity, InterfaceWirelessArp, InterfaceWirelessBand,
        InterfaceWirelessBasicRatesAG, InterfaceWirelessByDefaultName, InterfaceWirelessCfg,
        InterfaceWirelessChannelWidth, InterfaceWirelessFrequencyMode,
        InterfaceWirelessGuardInterval, InterfaceWirelessHwProtectionMode,
        InterfaceWirelessInstallation, InterfaceWirelessMulticastHelper, InterfaceWirelessNv2Mode,
        InterfaceWirelessNv2Qos, InterfaceWirelessPreambleMode, InterfaceWirelessRateSelection,
        InterfaceWirelessRateSet, InterfaceWirelessSupportedRatesAG, InterfaceWirelessTxPowerMode,
        InterfaceWirelessVlanMode, InterfaceWirelessWdsMode, InterfaceWirelessWpsMode,
    },
    value::{Auto, PossibleRangeDash},
};
use std::{collections::BTreeSet, iter::repeat_n, time::Duration};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub enum WLanMethod {
    #[default]
    None,
    Capsman,
    Wifi,
}

pub fn build_ethernet_ports(model: &[u8]) -> Box<[InterfaceEthernetByDefaultName]> {
    match model {
        b"RB962UiGS-5HacT2HnT" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1598, false),
            4,
        )
        .chain(repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1598, true),
            1,
        ))
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::Sfp, &ADVERTISE_1G_SFP, 1600, false),
                1,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"RB750Gr3" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1596, false),
            5,
        )
        .enumerate()
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"CRS326-24G-2S+" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1592, false),
            24,
        )
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::SfpSfpPlus, &ADVERTISE_10G, 1592, false),
                2,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"CRS318-16P-2S+" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1592, true),
            16,
        )
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::SfpSfpPlus, &ADVERTISE_10G, 1592, false),
                2,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"C52iG-5HaxD2HaxD" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1568, true),
            1,
        )
        .chain(repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1568, false),
            4,
        ))
        .enumerate()
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"cAPGi-5HaxD2HaxD" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1568, false),
            1,
        )
        .chain(repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1568, true),
            1,
        ))
        .enumerate()
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"CCR1009-7G-1C-1S+" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G_FULL, 1580, false),
            7,
        )
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::Combo, &ADVERTISE_1G_FULL, 1580, false),
                1,
            )
            .enumerate(),
        )
        .chain(
            repeat_n(
                generate_ethernet(
                    EthernetNamePattern::SfpSfpPlus,
                    &ADVERTISE_10G_FULL,
                    1580,
                    false,
                ),
                1,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"CRS354-48G-4S+2Q+" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1592, false),
            48,
        )
        .chain(repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_100M, 1592, false),
            1,
        ))
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::SfpSfpPlus, &ADVERTISE_10G, 1592, false),
                4,
            )
            .enumerate(),
        )
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::QsfpPlus, &ADVERTISE_10G, 1592, false),
                4 * 2,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        b"CRS109-8G-1S-2HnD" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1588, false),
            8,
        )
        .enumerate()
        .chain(
            repeat_n(
                generate_ethernet(EthernetNamePattern::Sfp, &ADVERTISE_1G_SFP, 1588, false),
                1,
            )
            .enumerate(),
        )
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        // point-to-point radio bridges: one ethernet port next to the radio
        b"RBDynaDishG-5HacDr3"
        | b"SXT G-5HPacD"
        | b"RBwAPG-60ad"
        | b"RBwAPG-60ad-A"
        | b"RBLHGG-60ad" => repeat_n(
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1598, false),
            1,
        )
        .enumerate()
        .map(|(idx, generator)| generator(idx + 1))
        .collect(),
        _ => Box::default(),
    }
}
pub fn find_wlan_method(model: &[u8]) -> WLanMethod {
    match model {
        b"cAPGi-5HaxD2HaxD" => WLanMethod::Wifi,
        _ => WLanMethod::default(),
    }
}
pub fn build_wifi_ports(model: &[u8]) -> Box<[InterfaceWifiByDefaultName]> {
    match model {
        b"C52iG-5HaxD2HaxD" => repeat_n(generate_wifi(), 2)
            .enumerate()
            .map(|(idx, generator)| generator(idx + 1))
            .collect(),
        &_ => Box::default(),
    }
}
pub fn build_wireless_ports(model: &[u8]) -> Box<[InterfaceWirelessByDefaultName]> {
    match model {
        b"CRS109-8G-1S-2HnD" => repeat_n(generate_wlan(1600), 1)
            .enumerate()
            .map(|(idx, generator)| generator(idx + 1))
            .collect(),
        // integrated antenna gain as the boards report it
        b"RBDynaDishG-5HacDr3" => [wireless_bridge_port(1, 25)].into(),
        b"SXT G-5HPacD" => [wireless_bridge_port(1, 14)].into(),
        &_ => Box::default(),
    }
}

/// A 5 GHz radio `wlan{idx}` (`/interface/wireless`) of a point-to-point bridge
/// board, with the integrated antenna's gain in dBi.
///
/// Every field the radio reports is set: RouterOS's factory settings (as on
/// `link-5-ap`), except no WPS, no WDS (`station-bridge` doesn't need it) and no
/// roaming, as an end of a bridge has exactly one peer. Fields the radio doesn't
/// report (`antenna-mode` with an integrated antenna, the 2.4 GHz rates, ...) stay
/// empty. `WirelessDeviceDataTarget::generate_from` adds role, channel, link and
/// regulatory settings - or replaces all of it with the radio's current
/// configuration if NetBox gives it no RF role.
pub fn wireless_bridge_port(idx: usize, antenna_gain: u32) -> InterfaceWirelessByDefaultName {
    let name: AsciiString = format!("wlan{idx}").into();
    let mcs = |prefix: &str, range: std::ops::Range<u8>| -> BTreeSet<AsciiString> {
        range.map(|n| format!("{prefix}{n}").into()).collect()
    };
    InterfaceWirelessByDefaultName {
        default_name: Some(name.clone()),
        data: InterfaceWirelessCfg {
            adaptive_noise_immunity: Some(InterfaceWirelessAdaptiveNoiseImmunity::None),
            allow_sharedkey: Some(false),
            ampdu_priorities: Some([0].into()),
            amsdu_limit: Some(8192),
            amsdu_threshold: Some(8192),
            antenna_gain: Some(antenna_gain),
            antenna_mode: None,
            area: None,
            arp: Some(InterfaceWirelessArp::Enabled),
            arp_timeout: None,
            band: Some(InterfaceWirelessBand::_5GhzOnlyac),
            basic_rates_a_g: Some([InterfaceWirelessBasicRatesAG::_6Mbps].into()),
            basic_rates_b: None,
            bridge_mode: Some(EnabledDisabled::Enabled),
            burst_time: None,
            channel_width: Some(InterfaceWirelessChannelWidth::_204080MhzXxxx),
            comment: None,
            compression: Some(false),
            country: None,
            default_ap_tx_limit: Some(0),
            default_authentication: Some(true),
            default_client_tx_limit: Some(0),
            default_forwarding: Some(true),
            disable_running_check: Some(false),
            disabled: false,
            disconnect_timeout: Some(Duration::from_secs(3)),
            distance: Some("dynamic".into()),
            frame_lifetime: Some(0),
            frequency: Some(Auto::Auto),
            frequency_mode: Some(InterfaceWirelessFrequencyMode::RegulatoryDomain),
            frequency_offset: Some(0),
            guard_interval: Some(InterfaceWirelessGuardInterval::Any),
            hide_ssid: Some(false),
            ht_basic_mcs: Some(mcs("mcs-", 0..8)),
            ht_supported_mcs: Some(mcs("mcs-", 0..24)),
            hw_fragmentation_threshold: Some("disabled".into()),
            hw_protection_mode: Some(InterfaceWirelessHwProtectionMode::None),
            hw_protection_threshold: Some(0),
            hw_retries: Some(7),
            installation: Some(InterfaceWirelessInstallation::Outdoor),
            interworking_profile: None,
            keepalive_frames: Some(EnabledDisabled::Enabled),
            l_2_mtu: None,
            mac_address: None,
            master_interface: None,
            max_station_count: Some(2007),
            mode: None,
            mtu: Some(1500),
            multicast_buffering: Some(EnabledDisabled::Enabled),
            multicast_helper: Some(InterfaceWirelessMulticastHelper::Default),
            name,
            noise_floor_threshold: None,
            nv_2_cell_radius: Some(30),
            nv_2_downlink_ratio: Some("50".into()),
            nv_2_mode: Some(InterfaceWirelessNv2Mode::DynamicDownlink),
            nv_2_noise_floor_offset: None,
            nv_2_preshared_key: None,
            nv_2_qos: Some(InterfaceWirelessNv2Qos::Default),
            nv_2_queue_count: Some(2),
            nv_2_security: Some(EnabledDisabled::Disabled),
            nv_2_sync_secret: None,
            on_fail_retry_time: Some(Duration::from_millis(100)),
            preamble_mode: Some(InterfaceWirelessPreambleMode::Both),
            prism_cardtype: None,
            radio_name: None,
            rate_selection: Some(InterfaceWirelessRateSelection::Advanced),
            rate_set: Some(InterfaceWirelessRateSet::Default),
            rx_chains: Some([0, 1].into()),
            scan_list: Some("default".into()),
            secondary_frequency: None,
            security_profile: Some("default".into()),
            skip_dfs_channels: Some("disabled".into()),
            ssid: None,
            station_bridge_clone_mac: Some(MacAddress::new([0; 6])),
            station_roaming: Some(EnabledDisabled::Disabled),
            supported_rates_a_g: Some({
                use InterfaceWirelessSupportedRatesAG::*;
                [
                    _6Mbps, _9Mbps, _12Mbps, _18Mbps, _24Mbps, _36Mbps, _48Mbps, _54Mbps,
                ]
                .into()
            }),
            supported_rates_b: None,
            tdma_period_size: Some(Auto::Auto),
            tx_chains: Some([0, 1].into()),
            tx_power: None,
            tx_power_mode: Some(InterfaceWirelessTxPowerMode::Default),
            update_stats_interval: Some("disabled".into()),
            vht_basic_mcs: Some("mcs0-7".into()),
            vht_supported_mcs: Some("mcs0-9,mcs0-9,mcs0-9".into()),
            vlan_id: Some(1),
            vlan_mode: Some(InterfaceWirelessVlanMode::NoTag),
            wds_cost_range: Some(PossibleRangeDash::Range {
                start: 50,
                end: 150,
            }),
            wds_default_bridge: Some("none".into()),
            wds_default_cost: Some("100".into()),
            wds_ignore_ssid: Some(false),
            wds_mode: Some(InterfaceWirelessWdsMode::Disabled),
            wireless_protocol: None,
            wmm_support: Some(EnabledDisabled::Disabled),
            wps_mode: Some(InterfaceWirelessWpsMode::Disabled),
        },
    }
}

pub fn build_w60g_ports(model: &[u8]) -> Box<[InterfaceW60GByDefaultName]> {
    match model {
        b"RBwAPG-60ad" | b"RBwAPG-60ad-A" | b"RBLHGG-60ad" => [w60g_port(1)].into(),
        &_ => Box::default(),
    }
}

/// A 60 GHz radio `wlan60-{idx}` (`/interface/w60g`) of a point-to-point bridge
/// board, with every field the radio reports set to its factory value (as on the
/// production links). `W60GDeviceDataTarget::generate_from` adds role, channel,
/// link and region - or replaces all of it with the radio's current configuration
/// if NetBox gives it no RF role.
pub fn w60g_port(idx: usize) -> InterfaceW60GByDefaultName {
    let name: AsciiString = format!("wlan60-{idx}").into();
    InterfaceW60GByDefaultName {
        default_name: name.clone(),
        data: InterfaceW60GCfg {
            name,
            comment: None,
            disabled: false,
            mode: None,
            ssid: None,
            password: None,
            frequency: Some(Auto::Auto),
            region: None,
            put_stations_in_bridge: None,
            isolate_stations: Some(false),
            tx_sector: Some(Auto::Auto),
            scan_list: None,
            arp: Some(InterfaceW60GArp::Enabled),
            arp_timeout: Some(Auto::Auto),
            mtu: Some(1500),
            l_2_mtu: None,
        },
    }
}
