//! Scope: What is specific to a board model (ports, switch chips, PoE, radio packages); nothing
//! that depends on NetBox.
use crate::topology::PhysicalPortId;
use mikrotik_model::{
    MacAddress,
    ascii::AsciiString,
    hwconfig::{
        ADVERTISE_1G, ADVERTISE_1G_FULL, ADVERTISE_1G_SFP, ADVERTISE_10G, ADVERTISE_10G_FULL,
        ADVERTISE_25G, ADVERTISE_100M, EthernetNamePattern, generate_ethernet, generate_wlan,
    },
    model::{
        Arp, EnabledDisabled, EthernetSpeed, InterfaceEthernetByDefaultName, InterfaceW60GByName,
        InterfaceW60GCfg, InterfaceWirelessAdaptiveNoiseImmunity, InterfaceWirelessBasicRatesAG,
        InterfaceWirelessByDefaultName, InterfaceWirelessCfg, InterfaceWirelessChannelWidth,
        InterfaceWirelessFrequencyMode, InterfaceWirelessNv2Mode, InterfaceWirelessNv2Qos,
        InterfaceWirelessPreambleMode, InterfaceWirelessRateSelection, InterfaceWirelessRateSet,
        InterfaceWirelessSupportedRatesAG, InterfaceWirelessTxPowerMode, InterfaceWirelessWdsMode,
        InterfaceWirelessWpsMode, WirelessBand, WirelessGuardInterval, WirelessInstallation,
        WirelessMulticastHelper, WirelessVlanMode,
    },
    value::{Auto, HasDisabled, HasNone, PossibleRangeDash},
};
use std::{collections::BTreeSet, ops::RangeInclusive, time::Duration};

/// The RouterOS 7 package a board's radios need: `wireless` for the 802.11 radios of the old
/// stack (`/interface/wireless`) and the 60 GHz ones (`/interface/w60g`), `wifi-qcom` or
/// `wifi-qcom-ac` for the newer `/interface/wifi` stack. A wAP ac also runs `wireless`
/// (`runs_wireless`), as a CAP of the legacy CAPsMAN.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum RadioPackage {
    Wireless,
    WifiQcom,
    WifiQcomAc,
}

impl RadioPackage {
    pub fn name(self) -> &'static str {
        match self {
            RadioPackage::Wireless => "wireless",
            RadioPackage::WifiQcom => "wifi-qcom",
            RadioPackage::WifiQcomAc => "wifi-qcom-ac",
        }
    }
    /// Whether it brings the radios of `/interface/wifi`.
    pub fn is_wifi(self) -> bool {
        matches!(self, RadioPackage::WifiQcom | RadioPackage::WifiQcomAc)
    }
    /// Whether its radios tag the clients' traffic themselves (`datapath.vlan-id`). The
    /// 802.11ac chips of `wifi-qcom-ac` don't ("802.11ac chipsets do not support this type of
    /// VLAN tagging", MikroTik's WiFi manual); a radio with `vlan-id` there drops all traffic
    /// (seen on a wAP ac, 7.23.8), it has to be an access port of the bridge instead.
    pub fn tags_vlans(self) -> bool {
        matches!(self, RadioPackage::WifiQcom)
    }
}

/// Whether `a` and `b` name the same board: case, spaces and a leading `RB` don't count, as
/// RouterOS 6 reports some boards without it (`912UAG-2HPnD`). A revision does
/// (`SXT G-5HPacD r2` is not `RBSXTG-5HPacD`).
pub fn same_board(a: &str, b: &str) -> bool {
    let plain = |model: &str| {
        let model = model
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_uppercase();
        model.strip_prefix("RB").map(str::to_owned).unwrap_or(model)
    };
    plain(a) == plain(b)
}

/// A port's PoE out counts as high voltage from this on (`poe-out-voltage`): the boards' limits
/// are given below and above 30 V.
pub const POE_HIGH_VOLTAGE: f64 = 30.0;

/// Ports whose PoE out shares one current limit, as the product page gives it, in mA: `low`
/// for the ports running below [`POE_HIGH_VOLTAGE`], `high` above (`None`: the board feeds no
/// high voltage). A netPower switches the voltage per port and has no converter: 24 V comes
/// from a PSU at its first jack, 48 V from one at its second, both only with two PSUs (lab-np2:
/// `psu1-voltage` 24.2, `psu2-voltage` 48.5). So one group can run both, each from its own PSU,
/// and the limits are taken as separate (the manual doesn't say how they combine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoeGroup {
    /// the `etherN` numbers of the ports
    pub ports: RangeInclusive<u8>,
    pub low: u32,
    pub high: Option<u32>,
}

/// The PoE out groups of the board (mikrotik.com product pages and manual, 2026-11-13); empty
/// for a board without PoE out, or one whose limits aren't entered yet.
pub fn poe_groups(model: &[u8]) -> &'static [PoeGroup] {
    const fn group(ports: RangeInclusive<u8>, low: u32, high: Option<u32>) -> PoeGroup {
        PoeGroup { ports, low, high }
    }
    match model {
        // 2.8 A per 8-port group below 30 V, 1.4 A above; which ports form a group the manual
        // doesn't say
        b"CRS318-16P-2S+" => {
            const {
                &[
                    group(1..=8, 2800, Some(1400)),
                    group(9..=16, 2800, Some(1400)),
                ]
            }
        }
        // hEX PoE: 2 A in total
        b"RB960PGS" => const { &[group(2..=5, 2000, Some(2000))] },
        // hAP ac: ether5, 700 mA, 350 mA above 30 V
        b"RB962UiGS-5HacT2HnT" => const { &[group(5..=5, 700, Some(350))] },
        // hAP ax²: ether1, 600 mA; it takes 18-28 V only
        b"C52iG-5HaxD2HaxD" => const { &[group(1..=1, 600, None)] },
        // cAP ax: ether2, 600 mA, 400 mA above 30 V
        b"cAPGi-5HaxD2HaxD" => const { &[group(2..=2, 600, Some(400))] },
        _ => &[],
    }
}

/// The package for the board's radios; `None` for a board without any.
pub fn radio_package(model: &[u8]) -> Option<RadioPackage> {
    match model {
        b"cAPGi-5HaxD2HaxD" | b"C52iG-5HaxD2HaxD" => Some(RadioPackage::WifiQcom),
        b"RBwAPG-5HacD2HnD" => Some(RadioPackage::WifiQcomAc),
        b"CRS109-8G-1S-2HnD"
        | b"RB962UiGS-5HacT2HnT"
        // the first wAP ac (mipsbe): no `wifi-qcom-ac` for it
        | b"RBwAPG-5HacT2HnD"
        // cAP 2n (mipsbe, 2.4 GHz 802.11n): a CAP of the legacy CAPsMAN
        | b"RBcAP2n"
        // Groove A-52HPn (mipsbe, one 802.11n radio for 2.4 or 5 GHz): a CAP of the legacy
        // CAPsMAN as groove-38 runs it (7.14.1), RouterOS reports it with a space
        | b"Groove A-52HPn"
        | b"RBDynaDishG-5HacDr3"
        // RouterOS 6 reports the RB912 without `RB`
        | b"RB912UAG-2HPnD"
        | b"912UAG-2HPnD"
        | b"SXT G-5HPacD"
        | b"SXT G-5HPacD r2"
        | b"RBwAPG-60ad"
        | b"RBwAPG-60ad-A"
        | b"RBLHGG-60ad" => Some(RadioPackage::Wireless),
        _ => None,
    }
}

/// The family of a board's switch chip, by what its bridge hardware offloading supports
/// (MikroTik's manual, "Bridge Hardware Offloading" and the board table of "Switch Chip
/// Features", 2026-10-30; `/interface/ethernet/switch` reports the chip). A CRS1xx/2xx offloads
/// several bridges, every other chip only one; what a chip doesn't offload turns the offloading
/// of that bridge off:
///
/// | family | VLAN filtering | IGMP/MLD snooping |
/// |---|---|---|
/// | `Prestera` | yes | yes |
/// | `Marvell88E6` | yes | yes |
/// | `MediaTek` | yes | no |
/// | `Crs1xx` | in the switch menus | yes |
/// | `Qca8337` | no | no |
/// | `Atheros8227` | no | no |
/// | `IpqPpe` | nothing is offloaded | |
///
/// STP/RSTP is offloaded everywhere but on `IpqPpe`, MSTP where VLAN filtering is, horizon
/// nowhere.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SwitchChip {
    /// Marvell Prestera (98DX…): CRS3xx/4xx/5xx/8xx, CCR2116, CCR2216, RDS2216; a CRS318-16P-2S+
    /// reports `Marvell-98DX226S`
    Prestera,
    /// 88E6393X, 88E6191X, 88E6190: RB5009, L009, CCR2004-16G-2S+
    Marvell88E6,
    /// MT7621, MT7531, EN7523, RTL8367: hEX, hEX S, hAP ax lite, RB4011, RB1100AHx4
    MediaTek,
    /// the switch of a CRS1xx/2xx, configured under `/interface/ethernet/switch`; a
    /// CRS109-8G-1S-2HnD reports `QCA-8513L` with all ports but the radio
    Crs1xx,
    /// QCA8337, Atheros8327, Atheros8316: hAP ac, hEX PoE, wAP ac, CCR1009-8G; a wAP ac and a
    /// wAP 60G report `Atheros-8327`
    Qca8337,
    /// Atheros8227, Atheros7240: hAP lite, hEX lite, hAP ac lite
    Atheros8227,
    /// IPQ-PPE, QCA8386, 98PX1012: hAP ax², hAP ax³, cAP ax, CCR2004-1G-12S+2XS (reports
    /// `Marvell-98PX1012`)
    IpqPpe,
}

impl SwitchChip {
    /// Whether it offloads a bridge at all.
    pub fn offloads_bridges(self) -> bool {
        !matches!(self, SwitchChip::IpqPpe)
    }
    /// Whether a VLAN-filtering bridge stays in hardware.
    pub fn offloads_vlan_filtering(self) -> bool {
        matches!(
            self,
            SwitchChip::Prestera | SwitchChip::Marvell88E6 | SwitchChip::MediaTek
        )
    }
    /// Whether VLANs belong in the switch's own menus (`/interface/ethernet/switch/vlan`,
    /// `egress-vlan-tag`, `ingress-vlan-translation`): a VLAN-filtering bridge runs on the CPU
    /// there (E17; MikroTik's manual „Basic VLAN switching“, 7.24).
    pub fn vlans_in_switch_menu(self) -> bool {
        matches!(self, SwitchChip::Crs1xx)
    }
    /// Whether a bridge with IGMP/MLD snooping stays in hardware.
    pub fn offloads_igmp_snooping(self) -> bool {
        matches!(
            self,
            SwitchChip::Prestera | SwitchChip::Marvell88E6 | SwitchChip::Crs1xx
        )
    }
}

/// The ports of a board on one switch chip; a port on none (`sfp1` of a hAP ac, `ether1` of
/// an L009) goes to the CPU and is never offloaded.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ChipPorts {
    /// every port that isn't a radio
    All,
    /// the `ether<N>` of these numbers
    Ether(RangeInclusive<u16>),
    /// the `ether<N>` of these numbers and `sfp1`
    EtherAndSfp1(RangeInclusive<u16>),
}

impl ChipPorts {
    fn contains(&self, port: PhysicalPortId) -> bool {
        match (self, port) {
            (_, port) if port.is_radio() || port == PhysicalPortId::Loopback => false,
            (ChipPorts::All, _) => true,
            (
                ChipPorts::Ether(numbers) | ChipPorts::EtherAndSfp1(numbers),
                PhysicalPortId::Ethernet(n),
            ) => numbers.contains(&n),
            (ChipPorts::EtherAndSfp1(..), PhysicalPortId::Sfp(1)) => true,
            _ => false,
        }
    }
}

/// The switch chips of a board and their ports, from MikroTik's board table (`SwitchChip`),
/// by the model `/system/routerboard` reports. Empty for a board without one (an SXT) or one
/// the table doesn't list: such a board keeps the settings every board gets.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct SwitchChips(&'static [(SwitchChip, ChipPorts)]);

impl SwitchChips {
    pub fn of(model: &[u8]) -> Self {
        use ChipPorts::{All, Ether, EtherAndSfp1};
        use SwitchChip::*;
        const PRESTERA: &[&[u8]] = &[
            b"CRS3", b"CRS4", b"CRS5", b"CRS8", b"CCR2116", b"CCR2216", b"RDS2216",
        ];
        let prefixed = |prefixes: &[&[u8]]| prefixes.iter().any(|prefix| model.starts_with(prefix));
        Self(if prefixed(PRESTERA) {
            &[(Prestera, All)]
        } else if prefixed(&[b"CRS1", b"CRS2"]) {
            &[(Crs1xx, All)]
        } else if prefixed(&[b"RB5009"]) {
            &[(Marvell88E6, All)]
        } else if prefixed(&[b"L009"]) {
            &[(Marvell88E6, EtherAndSfp1(2..=8))]
        } else if prefixed(&[b"RB3011"]) {
            &[(Qca8337, Ether(1..=10))]
        } else if prefixed(&[b"RB2011"]) {
            &[(Qca8337, EtherAndSfp1(1..=5)), (Atheros8227, Ether(6..=10))]
        } else {
            match model {
                b"CCR2004-16G-2S+" => &[(Marvell88E6, Ether(1..=16))],
                b"RB750Gr3" => &[(MediaTek, Ether(1..=5))],
                b"RB760iGS" => &[(MediaTek, Ether(1..=5))],
                b"RBM33G" => &[(MediaTek, Ether(1..=3))],
                b"L41G-2axD" => &[(MediaTek, Ether(1..=4))],
                b"E50UG" => &[(MediaTek, Ether(2..=5))],
                b"RB4011iGS+" => &[(MediaTek, Ether(1..=10))],
                b"RB1100AHx4" => &[(MediaTek, Ether(1..=13))],
                b"RB750Gr2"
                | b"RB962UiGS-5HacT2HnT"
                | b"RB960PGS"
                | b"RB960PGS-PB"
                | b"RB750GL"
                | b"RB751G-2HnD"
                | b"RB951G-2HnD"
                | b"RBD52G-5HacD2HnD"
                | b"RBD53iG-5HacD2HnD"
                | b"RB850Gx2"
                | b"RB450G"
                | b"RB450Gx4"
                | b"RB750G"
                | b"RB1200" => &[(Qca8337, Ether(1..=5))],
                b"RBcAPGi-5acD2nD"
                | b"RBwAPGR-5HacD2HnD"
                | b"RBwAPG-5HacD2HnD"
                | b"RBD25G-5HPacQD2HPnD" => &[(Qca8337, Ether(1..=2))],
                // not in the table, but reports `Atheros-8327` (7.23.8)
                b"RBwAPG-60ad" => &[(Qca8337, Ether(1..=1))],
                b"RB953GS" => &[(Qca8337, EtherAndSfp1(1..=3))],
                b"RBD22UGS-5HPacD2HnD" => &[(Qca8337, EtherAndSfp1(1..=1))],
                b"RB1100AH" | b"RB1100AHx2" | b"RB1100" => &[(Qca8337, Ether(1..=10))],
                b"CCR1009-8G-1S-1S+" | b"CCR1009-8G-1S" => &[(Qca8337, Ether(1..=4))],
                b"RB493G" => &[(Qca8337, Ether(1..=9))],
                b"RB435G" | b"RB433GL" => &[(Qca8337, Ether(1..=3))],
                b"RB941-2nD" => &[(Atheros8227, Ether(1..=4))],
                b"RB951Ui-2nD" | b"RB952Ui-5ac2nD" | b"RB750r2" | b"RB750UPr2" | b"RB750P-PBr2"
                | b"RB951Ui-2HnD" => &[(Atheros8227, Ether(1..=5))],
                b"RBwsAP-5Hac2nD" => &[(Atheros8227, Ether(1..=3))],
                b"RBmAP2nD" | b"RBmAP2n" => &[(Atheros8227, Ether(1..=2))],
                b"RB750" | b"RB750UP" | b"RB751U-2HnD" | b"RB951-2n" => {
                    &[(Atheros8227, Ether(2..=5))]
                }
                b"C52iG-5HaxD2HaxD" | b"C52iG-5HaxD2HaxD-TC" | b"C53UiG+5HPaxD2HPaxD" => {
                    &[(IpqPpe, Ether(1..=5))]
                }
                b"cAPGi-5HaxD2HaxD" => &[(IpqPpe, Ether(1..=2))],
                b"MA53UG+HbeH" => &[(IpqPpe, Ether(2..=5))],
                b"CCR2004-1G-12S+2XS" => &[(IpqPpe, All)],
                // no switch chip: every port on the CPU (srv-router-berg, 7.23.8)
                b"CCR1009-7G-1C-1S+" => &[],
                _ => &[],
            }
        })
    }
    /// The chip `port` is on, `None` for a radio or a port of the CPU.
    /// Whether a chip of the board takes VLANs in its own menus
    /// ([`SwitchChip::vlans_in_switch_menu`]): the device has those menus to read.
    pub fn have_switch_menu_vlans(self) -> bool {
        self.0.iter().any(|(chip, _)| chip.vlans_in_switch_menu())
    }
    pub fn chip_of(self, port: PhysicalPortId) -> Option<SwitchChip> {
        self.0
            .iter()
            .find(|(_, ports)| ports.contains(port))
            .map(|(chip, _)| *chip)
    }
}

/// Whether the board's `wifi` radios also run with `wireless` (as `wlan<N>`), which NetBox
/// asks for with `wlan<N>` ports (`needed_radio_package`): a wAP ac can then be a CAP of the
/// legacy CAPsMAN, whose CAPs tag VLANs (its `wifi-qcom-ac` radios can't).
pub fn runs_wireless(model: &[u8]) -> bool {
    matches!(model, b"RBwAPG-5HacD2HnD")
}

/// The factory values of the power cycling of a hung PD, which not every PoE board has, as a
/// CRS318-16P-2S+ (RouterOS 7.19), a hAP ax² and a cAP ax (7.23.8) report them. Without them the
/// diff would clear them, which RouterOS refuses for `power-cycle-ping-enabled` (must be yes or
/// no).
fn with_poe_power_cycle(
    mut port: InterfaceEthernetByDefaultName,
) -> InterfaceEthernetByDefaultName {
    port.data.power_cycle_interval = Some(HasNone::NoneValue);
    port.data.power_cycle_ping_enabled = Some(false);
    port
}

/// The power cycling and the voltage selection, as a CRS318-16P-2S+ reports them; a hAP ax²
/// has no `poe-voltage`.
fn with_poe_power_control(port: InterfaceEthernetByDefaultName) -> InterfaceEthernetByDefaultName {
    let mut port = with_poe_power_cycle(port);
    port.data.poe_voltage = Some(Auto::Auto);
    port
}

/// What a port gives PoE out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PoeOut {
    No,
    /// with the power cycling of a hung PD (`with_poe_power_cycle`)
    PowerCycle,
    /// with the power cycling and the voltage selection (`with_poe_power_control`)
    PowerControl,
}

/// A run of alike ports of a board, numbered on from the run before of the same name pattern.
#[derive(Debug, Clone, Copy)]
struct Ports {
    name: EthernetNamePattern,
    speeds: &'static [EthernetSpeed],
    l2mtu: u16,
    poe_out: PoeOut,
    count: usize,
}

const fn ports(
    name: EthernetNamePattern,
    speeds: &'static [EthernetSpeed],
    l2mtu: u16,
    count: usize,
) -> Ports {
    Ports {
        name,
        speeds,
        l2mtu,
        poe_out: PoeOut::No,
        count,
    }
}

const fn poe_out(poe_out: PoeOut, ports: Ports) -> Ports {
    Ports { poe_out, ..ports }
}

/// The ethernet ports of the runs, in their order.
fn build_runs(runs: &[Ports]) -> Box<[InterfaceEthernetByDefaultName]> {
    let mut numbers: Vec<(EthernetNamePattern, usize)> = Vec::new();
    let mut built = Vec::new();
    for run in runs {
        let first = match numbers.iter_mut().find(|(name, _)| *name == run.name) {
            Some((_, last)) => last,
            None => &mut numbers.push_mut((run.name, 0)).1,
        };
        let generator =
            generate_ethernet(run.name, run.speeds, run.l2mtu, run.poe_out != PoeOut::No);
        built.extend(
            (*first + 1..=*first + run.count).map(|idx| match run.poe_out {
                PoeOut::No => generator(idx),
                PoeOut::PowerCycle => with_poe_power_cycle(generator(idx)),
                PoeOut::PowerControl => with_poe_power_control(generator(idx)),
            }),
        );
        *first += run.count;
    }
    built.into()
}

/// The ethernet ports of a board, by the model `/system/routerboard` reports; none for a board
/// not listed.
pub fn build_ethernet_ports(model: &[u8]) -> Box<[InterfaceEthernetByDefaultName]> {
    use EthernetNamePattern::{Combo, Ether, QsfpPlus, Sfp, Sfp28, SfpSfpPlus};
    use PoeOut::{PowerControl, PowerCycle};
    let runs: &[Ports] = match model {
        // hAP ac: PoE-out on ether5, with the power cycling of a hung PD (ap-klr-solar, 7.23.7)
        b"RB962UiGS-5HacT2HnT" => &[
            ports(Ether, &ADVERTISE_1G, 1598, 4),
            poe_out(PowerCycle, ports(Ether, &ADVERTISE_1G, 1598, 1)),
            ports(Sfp, &ADVERTISE_1G_SFP, 1600, 1),
        ],
        // hEX PoE: ether1 is PoE-in only, PoE-out on ether2-5, a 1G SFP cage (l2mtu as
        // router-schopf reports them, 7.23.8)
        b"RB960PGS" => &[
            ports(Ether, &ADVERTISE_1G, 1598, 1),
            poe_out(PowerCycle, ports(Ether, &ADVERTISE_1G, 1598, 4)),
            ports(Sfp, &ADVERTISE_1G_SFP, 1600, 1),
        ],
        // wAP ac: ether1 is PoE-in only
        b"RBwAPG-5HacD2HnD" => &[ports(Ether, &ADVERTISE_1G, 1598, 2)],
        // the first wAP ac: a single port, PoE-in only (7.23.7); Groove A-52HPn: a single port,
        // PoE-in only, a 10/100 port that advertises 1G all the same, no switch chip
        // (groove-38, 7.14.1); `SXT G-5HPacD r2` as RouterOS 7.20.8 reports it, with this l2mtu
        // (read from lab-sxt-a)
        b"RBwAPG-5HacT2HnD" | b"Groove A-52HPn" | b"SXT G-5HPacD r2" => {
            &[ports(Ether, &ADVERTISE_1G, 1600, 1)]
        }
        // cAP 2n: a single port, PoE-in only; a 10/100 port that advertises 1G all the same
        // (bachman-wlan-dg, 7.20.4); point-to-point radio bridges: one ethernet port next to
        // the radio
        b"RBcAP2n"
        | b"RBDynaDishG-5HacDr3"
        | b"RB912UAG-2HPnD"
        | b"912UAG-2HPnD"
        | b"SXT G-5HPacD"
        | b"RBwAPG-60ad"
        | b"RBwAPG-60ad-A"
        | b"RBLHGG-60ad" => &[ports(Ether, &ADVERTISE_1G, 1598, 1)],
        // RB750GL: five ports on an Atheros-8327, ether1 PoE-in only (sat-switch, 7.23.8)
        b"RB750GL" => &[ports(Ether, &ADVERTISE_1G, 1598, 5)],
        b"RB750Gr3" => &[ports(Ether, &ADVERTISE_1G, 1596, 5)],
        b"CRS326-24G-2S+" => &[
            ports(Ether, &ADVERTISE_1G, 1592, 24),
            ports(SfpSfpPlus, &ADVERTISE_10G, 1592, 2),
        ],
        // ether1 takes PoE in only; l2mtu 1584 on every port (klr-sw-3)
        b"CRS309-1G-8S+" => &[
            ports(Ether, &ADVERTISE_1G, 1584, 1),
            ports(SfpSfpPlus, &ADVERTISE_10G, 1584, 8),
        ],
        b"CRS318-16P-2S+" => &[
            poe_out(PowerControl, ports(Ether, &ADVERTISE_1G, 1592, 16)),
            ports(SfpSfpPlus, &ADVERTISE_10G, 1592, 2),
        ],
        // hAP ax²: PoE-out on ether1
        b"C52iG-5HaxD2HaxD" => &[
            poe_out(PowerCycle, ports(Ether, &ADVERTISE_1G, 1568, 1)),
            ports(Ether, &ADVERTISE_1G, 1568, 4),
        ],
        // cAP ax: PoE-out on ether2, with the power cycling of a hung PD (ap-schopf, 7.23.8)
        b"cAPGi-5HaxD2HaxD" => &[
            ports(Ether, &ADVERTISE_1G, 1568, 1),
            poe_out(PowerCycle, ports(Ether, &ADVERTISE_1G, 1568, 1)),
        ],
        b"CCR1009-7G-1C-1S+" => &[
            ports(Ether, &ADVERTISE_1G_FULL, 1580, 7),
            ports(Combo, &ADVERTISE_1G_FULL, 1580, 1),
            ports(SfpSfpPlus, &ADVERTISE_10G_FULL, 1580, 1),
        ],
        // router-faesigrund-keller (examples/02-edge-router-bgp-vpn), as its 7G sibling
        b"CCR1009-8G-1S-1S+" => &[
            ports(Ether, &ADVERTISE_1G_FULL, 1580, 8),
            ports(Sfp, &ADVERTISE_1G_FULL, 1580, 1),
            ports(SfpSfpPlus, &ADVERTISE_10G_FULL, 1580, 1),
        ],
        b"CCR2004-1G-12S+2XS" => &[
            ports(Ether, &ADVERTISE_1G, 1600, 1),
            ports(SfpSfpPlus, &ADVERTISE_10G, 1592, 12),
            ports(Sfp28, &ADVERTISE_25G, 1592, 2),
        ],
        // ether49 is the 100M management port
        b"CRS354-48G-4S+2Q+" => &[
            ports(Ether, &ADVERTISE_1G, 1592, 48),
            ports(Ether, &ADVERTISE_100M, 1592, 1),
            ports(SfpSfpPlus, &ADVERTISE_10G, 1592, 4),
            ports(QsfpPlus, &ADVERTISE_10G, 1592, 4 * 2),
        ],
        b"CRS109-8G-1S-2HnD" => &[
            ports(Ether, &ADVERTISE_1G, 1588, 8),
            ports(Sfp, &ADVERTISE_1G_SFP, 1588, 1),
        ],
        // lab-crs125 (7.14.1): PoE in on ether1, no PoE out
        b"CRS125-24G-1S" => &[
            ports(Ether, &ADVERTISE_1G, 1588, 24),
            ports(Sfp, &ADVERTISE_1G_SFP, 1588, 1),
        ],
        _ => &[],
    };
    build_runs(runs)
}
/// The `/interface/wifi` radios a board comes with (tests; the service reads them off the device).
#[cfg(test)]
pub fn build_wifi_ports(model: &[u8]) -> Box<[mikrotik_model::model::InterfaceWifiByDefaultName]> {
    use mikrotik_model::hwconfig::generate_wifi;
    match model {
        b"C52iG-5HaxD2HaxD" => (1..=2).map(generate_wifi()).collect(),
        &_ => Box::default(),
    }
}
pub fn build_wireless_ports(model: &[u8]) -> Box<[InterfaceWirelessByDefaultName]> {
    match model {
        b"CRS109-8G-1S-2HnD" | b"RB912UAG-2HPnD" | b"912UAG-2HPnD" => {
            [generate_wlan(1600)(1)].into()
        }
        // integrated antenna gain as the boards report it
        b"RBDynaDishG-5HacDr3" => [wireless_bridge_port(1, 25)].into(),
        b"SXT G-5HPacD" | b"SXT G-5HPacD r2" => [wireless_bridge_port(1, 14)].into(),
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
            arp: Some(Arp::Enabled),
            arp_timeout: Some(Auto::Auto),
            band: Some(WirelessBand::_5GhzOnlyac),
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
            guard_interval: Some(WirelessGuardInterval::Any),
            hide_ssid: Some(false),
            ht_basic_mcs: Some(mcs("mcs-", 0..8)),
            ht_supported_mcs: Some(mcs("mcs-", 0..24)),
            hw_fragmentation_threshold: Some("disabled".into()),
            hw_protection_mode: Some(HasNone::NoneValue),
            hw_protection_threshold: Some(0),
            hw_retries: Some(7),
            installation: Some(WirelessInstallation::Outdoor),
            interworking_profile: Some(EnabledDisabled::Disabled),
            keepalive_frames: Some(EnabledDisabled::Enabled),
            l_2_mtu: None,
            mac_address: None,
            master_interface: None,
            max_station_count: Some(2007),
            mode: None,
            mtu: Some(1500),
            multicast_buffering: Some(EnabledDisabled::Enabled),
            multicast_helper: Some(HasDisabled::Value(WirelessMulticastHelper::Default)),
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
            vlan_mode: Some(WirelessVlanMode::NoTag),
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

pub fn build_w60g_ports(model: &[u8]) -> Box<[InterfaceW60GByName]> {
    match model {
        b"RBwAPG-60ad" | b"RBwAPG-60ad-A" | b"RBLHGG-60ad" => [w60g_port(1)].into(),
        &_ => Box::default(),
    }
}

/// A 60 GHz radio `wlan60-{idx}` (`/interface/w60g`) of a point-to-point bridge
/// board, with every field the radio reports set to its factory value (as on the
/// production links). `W60GDeviceDataTarget::generate_from` adds role, channel,
/// link and region - or replaces all of it with the radio's current configuration
/// if NetBox gives it no RF role. Keyed by its name: `/interface/w60g` reports no
/// `default-name` (RouterOS 7.23).
pub fn w60g_port(idx: usize) -> InterfaceW60GByName {
    InterfaceW60GByName(InterfaceW60GCfg {
        name: format!("wlan60-{idx}").into(),
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
        arp: Some(Arp::Enabled),
        arp_timeout: Some(Auto::Auto),
        mtu: Some(1500),
        l_2_mtu: None,
    })
}

#[cfg(test)]
mod test;
