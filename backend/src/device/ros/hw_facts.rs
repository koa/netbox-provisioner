use mikrotik_model::{
    hwconfig::{
        ADVERTISE_1G, ADVERTISE_1G_FULL, ADVERTISE_1G_SFP, ADVERTISE_10G, ADVERTISE_10G_FULL,
        ADVERTISE_100M, EthernetNamePattern, generate_ethernet, generate_wifi, generate_wlan,
    },
    ascii::AsciiString,
    model::{
        InterfaceEthernetByDefaultName, InterfaceW60GByDefaultName, InterfaceW60GCfg,
        InterfaceWifiByDefaultName, InterfaceWirelessByDefaultName,
    },
};
use std::iter::repeat_n;

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
        b"RBDynaDishG-5HacDr3" | b"SXT G-5HPacD" => [wireless_port(1)].into(),
        &_ => Box::default(),
    }
}

/// A legacy wireless radio `wlan{idx}` (`/interface/wireless`) left as it is: only
/// the generator's explicit settings (`WirelessDeviceDataTarget::generate_from`)
/// change it, every other field is keep-if-none in the model.
pub fn wireless_port(idx: usize) -> InterfaceWirelessByDefaultName {
    let mut port = generate_wlan(1600)(idx);
    port.data.name = format!("wlan{idx}").into();
    port.data.l_2_mtu = None;
    port
}

pub fn build_w60g_ports(model: &[u8]) -> Box<[InterfaceW60GByDefaultName]> {
    match model {
        b"RBwAPG-60ad" | b"RBwAPG-60ad-A" | b"RBLHGG-60ad" => [w60g_port(1)].into(),
        &_ => Box::default(),
    }
}

/// A 60 GHz radio `wlan60-{idx}` (`/interface/w60g`), untouched apart from what
/// `W60gDeviceDataTarget::generate_from` sets.
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
            frequency: None,
            region: None,
            put_stations_in_bridge: None,
            isolate_stations: None,
            tx_sector: None,
            default_scan_list: None,
            scan_list: None,
            arp: None,
            arp_timeout: None,
            mtu: None,
            l_2_mtu: None,
        },
    }
}
