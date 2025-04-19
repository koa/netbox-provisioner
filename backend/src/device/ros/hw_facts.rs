use mikrotik_model::{
    hwconfig::{
        generate_ethernet, generate_wifi, generate_wlan, EthernetNamePattern, ADVERTISE_100M,
        ADVERTISE_10G, ADVERTISE_10G_FULL, ADVERTISE_1G, ADVERTISE_1G_FULL, ADVERTISE_1G_SFP,
    },
    model::{
        InterfaceEthernetByDefaultName, InterfaceWifiByDefaultName, InterfaceWirelessByDefaultName,
    },
};
use std::iter::repeat_n;

pub fn build_ethernet_ports(model: &[u8]) -> Box<[InterfaceEthernetByDefaultName]> {
    match model {
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
            generate_ethernet(EthernetNamePattern::Ether, &ADVERTISE_1G, 1592, false),
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
        _ => Box::default(),
    }
}
pub fn build_wifi_ports(model: &[u8]) -> Box<[InterfaceWifiByDefaultName]> {
    match model {
        b"C52iG-5HaxD2HaxD" => repeat_n(generate_wifi(1560), 2)
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
        &_ => Box::default(),
    }
}
