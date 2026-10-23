//! The radios: legacy `wireless`, `w60g`, `wifi` and the controllers (CAPsMAN), each in a
//! module of its own; here what several of them need.

use crate::{
    device::ros::{
        SetupError,
        hw_facts::{RadioPackage, radio_package, runs_wireless},
    },
    topology::{
        PhysicalPortId, RadioLink,
        access::{device::DeviceAccess, wlan_group::WlanGroupAccess},
    },
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{InterfaceW60GRegion, WifiCountry},
    value::{ParseRosValueResult, RosValue},
};
use std::collections::BTreeSet;

pub(crate) mod capsman;
pub(crate) mod w60g;
pub(crate) mod wifi;
pub(crate) mod wireless;

/// The key of an encrypted link, `None` for an open one.
fn link_key(link: &RadioLink) -> Result<Option<&str>, SetupError> {
    if !link.encrypted {
        return Ok(None);
    }
    link.psk
        .as_deref()
        .map(Some)
        .ok_or_else(|| SetupError::MissingPsk(link.ssid.clone()))
}

/// RouterOS's `/interface/wireless` country and `/interface/w60g` region for the
/// country of the device's site (the ISO 3166 code of its topmost region).
fn country_settings(
    device: &DeviceAccess,
) -> Result<(&'static str, InterfaceW60GRegion), SetupError> {
    use InterfaceW60GRegion::Eu;
    let code = device
        .country()
        .ok_or_else(|| SetupError::MissingCountry(device.name().into()))?;
    Ok(match code.to_ascii_lowercase().as_str() {
        "ch" => ("switzerland", Eu),
        "li" => ("liechtenstein", Eu),
        "de" => ("germany", Eu),
        "at" => ("austria", Eu),
        "fr" => ("france", Eu),
        "it" => ("italy", Eu),
        _ => return Err(SetupError::UnknownCountry(code.into())),
    })
}

/// The VLANs the WLANs of a group put their clients in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WlanVlans {
    /// one at most: the CAP sets it on its radios (or makes them access ports, `wifi-qcom-ac`)
    One(Option<u16>),
    /// several: the controller sets each WLAN's in its configuration, a CAP's radios tag them
    PerWlan,
}

fn wlan_vlans(group: &WlanGroupAccess) -> WlanVlans {
    let vlans = group
        .wlan()
        .filter_map(|wlan| wlan.vlan().and_then(|vlan| vlan.vlan_id()))
        .collect::<BTreeSet<_>>();
    match vlans.len() {
        0 | 1 => WlanVlans::One(vlans.first().copied()),
        _ => WlanVlans::PerWlan,
    }
}

/// The name of a WLAN's configuration (`/interface/wifi` and `/caps-man`): its SSID with
/// every character but ASCII letters, digits and `-` as `_`.
fn wlan_configuration_name(ssid: &str) -> AsciiString {
    ssid.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect::<String>()
        .into()
}

/// The package for the board's radios (`hw_facts::radio_package`) if NetBox uses them: a
/// radio with an RF role, in a bridge, with a VLAN or an address, or the device is an AP of a
/// Wireless LAN Group. A controller without radios of a group with CAPs of the legacy
/// CAPsMAN needs `wireless` for running it (`CapsManDeviceDataTarget`), and so does a board
/// whose radios also run with it (`hw_facts::runs_wireless`) where NetBox has `wlan<N>` ports.
/// A RouterOS update installs just that besides `routeros`.
pub fn needed_radio_package(device: &DeviceAccess, model: &[u8]) -> Option<RadioPackage> {
    let Some(mut package) = radio_package(model) else {
        return device
            .wlan_controller_of()
            .is_some_and(|group| group.has_legacy_caps())
            .then_some(RadioPackage::Wireless);
    };
    if runs_wireless(model)
        && device
            .interfaces()
            .iter()
            .any(|interface| matches!(interface.external_port(), Some(PhysicalPortId::Wlan(_))))
    {
        package = RadioPackage::Wireless;
    }
    let uses_radio = device.interfaces().iter().any(|interface| {
        matches!(
            interface.external_port(),
            Some(PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_) | PhysicalPortId::W60g(_))
        ) && (interface.radio().is_some()
            || interface.bridge().is_some()
            || !interface.ips().is_empty()
            || interface.untagged_vlan().is_some()
            || interface.tagged_vlans().next().is_some())
    });
    (uses_radio || device.wlan_ap_of().is_some()).then_some(package)
}

/// The RouterOS name of the device's country for `/interface/wifi` (capitalized, unlike the
/// legacy `/interface/wireless` one).
fn wifi_country(device: &DeviceAccess) -> Result<WifiCountry, SetupError> {
    let (name, _) = country_settings(device)?;
    let mut name = name.to_string();
    name[..1].make_ascii_uppercase();
    match WifiCountry::parse_ros(name.as_bytes()) {
        ParseRosValueResult::Value(country) => Ok(country),
        _ => Err(SetupError::UnknownCountry(name.into())),
    }
}
