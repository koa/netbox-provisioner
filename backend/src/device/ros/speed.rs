//! A port NetBox gives a `speed` runs at it, without auto-negotiation: a QSFP+ breakout lane
//! on a DAC (klr-sw-1 `qsfpplus1-1`, `auto-negotiation=no speed=10G-baseCR`) only comes up
//! that way. RouterOS names the speed with the medium (`10G-baseCR` copper twinax,
//! `10G-baseSR-LR` optics, `10G-baseT` an RJ45 module), which a cage leaves open; the NetBox
//! type of the port's cable tells it (`dac-passive`, `smf-os2`, `cat6` ...). Duplex only counts
//! for the RJ45 speeds up to 1G, half where NetBox says so.
//!
//! Without a `speed` the port negotiates, and RouterOS keeps whatever `speed` it has without
//! using it; it refuses one empty ("ambiguous value of speed"), so the model keeps it then
//! (`speed: k`).
use super::SetupError;
use crate::topology::{Duplex, PhysicalPortId, access::interface::InterfaceAccess};
use mikrotik_model::model::EthernetSpeed;

/// What the cable at a cage carries, from its NetBox type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Medium {
    Twisted,
    Twinax,
    Fiber,
}

fn medium(kind: &str) -> Option<Medium> {
    match kind {
        _ if kind.starts_with("cat") => Some(Medium::Twisted),
        "dac-active" | "dac-passive" => Some(Medium::Twinax),
        _ if kind.starts_with("mmf") || kind.starts_with("smf") || kind == "aoc" => {
            Some(Medium::Fiber)
        }
        _ => None,
    }
}

/// `kbits` as NetBox shows it (`10 Gbit/s`).
pub(crate) fn label(kbits: u32) -> String {
    match kbits {
        k if k >= 1_000_000 && k % 1_000_000 == 0 => format!("{} Gbit/s", k / 1_000_000),
        k if k >= 1_000_000 => format!("{} Gbit/s", k as f64 / 1_000_000.0),
        k if k >= 1_000 && k % 1_000 == 0 => format!("{} Mbit/s", k / 1_000),
        k => format!("{k} kbit/s"),
    }
}

/// The speed `port` runs at, as NetBox gives it; `None` where it negotiates.
pub(crate) fn fixed_speed(port: &InterfaceAccess) -> Result<Option<EthernetSpeed>, SetupError> {
    let Some(kbits) = port.speed() else {
        return Ok(None);
    };
    let name = || port.name().into();
    let half = port.duplex() == Some(Duplex::Half);
    let twisted = |half_speed, full_speed| if half { half_speed } else { full_speed };
    let speed = match port.external_port() {
        Some(PhysicalPortId::Ethernet(_)) => match kbits {
            10_000 => twisted(EthernetSpeed::_10MBaseTHalf, EthernetSpeed::_10MBaseTFull),
            100_000 => twisted(EthernetSpeed::_100MBaseTHalf, EthernetSpeed::_100MBaseTFull),
            1_000_000 => twisted(EthernetSpeed::_1GBaseTHalf, EthernetSpeed::_1GBaseTFull),
            2_500_000 => EthernetSpeed::_25GBaseT,
            5_000_000 => EthernetSpeed::_5GBaseT,
            10_000_000 => EthernetSpeed::_10GBaseT,
            _ => return Err(SetupError::UnknownSpeed { port: name(), speed: label(kbits) }),
        },
        Some(
            PhysicalPortId::Sfp(_)
            | PhysicalPortId::SfpSfpPlus(_)
            | PhysicalPortId::Sfp28(_)
            | PhysicalPortId::QsfpPlus(..)
            | PhysicalPortId::Combo(_),
        ) => {
            let medium = port
                .cable()
                .and_then(|cable| cable.kind().and_then(medium))
                .ok_or_else(|| SetupError::SpeedNeedsCableType { port: name(), speed: label(kbits) })?;
            use Medium::*;
            match (kbits, medium) {
                (100_000, Twisted) => twisted(EthernetSpeed::_100MBaseTHalf, EthernetSpeed::_100MBaseTFull),
                (100_000, Fiber) => {
                    if half { EthernetSpeed::_100MBaseFxHalf } else { EthernetSpeed::_100MBaseFxFull }
                }
                (1_000_000, Twisted) => twisted(EthernetSpeed::_1GBaseTHalf, EthernetSpeed::_1GBaseTFull),
                (1_000_000, Twinax | Fiber) => EthernetSpeed::_1GBaseX,
                (2_500_000, Twisted) => EthernetSpeed::_25GBaseT,
                (2_500_000, Twinax | Fiber) => EthernetSpeed::_25GBaseX,
                (5_000_000, Twisted) => EthernetSpeed::_5GBaseT,
                (10_000_000, Twisted) => EthernetSpeed::_10GBaseT,
                (10_000_000, Twinax) => EthernetSpeed::_10GBaseCr,
                (10_000_000, Fiber) => EthernetSpeed::_10GBaseSrLr,
                (25_000_000, Twinax) => EthernetSpeed::_25GBaseCr,
                (25_000_000, Fiber) => EthernetSpeed::_25GBaseSrLr,
                (40_000_000, Twinax) => EthernetSpeed::_40GBaseCr4,
                (40_000_000, Fiber) => EthernetSpeed::_40GBaseSr4Lr4,
                _ => return Err(SetupError::UnknownSpeed { port: name(), speed: label(kbits) }),
            }
        }
        _ => return Err(SetupError::SpeedNotSupported(name())),
    };
    // a fixed speed at one end and negotiation (or another speed) at the other: no link
    for far_end in port.connected_interfaces() {
        let routeros = far_end.device().is_some_and(|device| device.has_routeros());
        if routeros && far_end.speed() != Some(kbits) {
            return Err(SetupError::SpeedMismatch {
                port: name(),
                speed: label(kbits),
                far_end: format!(
                    "{} {}",
                    far_end.device().map(|d| d.name().to_string()).unwrap_or_default(),
                    far_end.name()
                )
                .into(),
                far_speed: far_end.speed().map(label).map(Into::into),
            });
        }
    }
    Ok(Some(speed))
}
