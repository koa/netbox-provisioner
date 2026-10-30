//! The legacy `/interface/wireless` radios: bridge links and CAPs of the legacy CAPsMAN.

use super::{country_settings, link_key};
use crate::{
    device::ros::{
        DEFAULT_MTU, SetupError, WirelessDeviceDataCurrent, WirelessDeviceDataTarget,
        hw_facts::build_wireless_ports,
        l2::{L2Setup, NameGenerator},
        l2mtu_for, port_mtu,
    },
    topology::{
        PhysicalPortId, Radio, RadioRole,
        access::device::DeviceAccess,
    },
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        EnabledDisabled, InterfaceWirelessCfg, InterfaceWirelessChannelWidth,
        InterfaceWirelessFrequencyMode, InterfaceWirelessMode, InterfaceWirelessTxPowerMode,
        InterfaceWirelessWirelessProtocol::{Nv2, Nv2Nstreme80211},
    },
    value::{Auto, HasNone, ParseRosValueResult, RosValue},
};
use std::collections::{HashMap, HashSet};

impl WirelessDeviceDataCurrent {
    /// The radios CAPsMAN manages, by default name with their name on the device: those the
    /// enabled CAP lists. RouterOS refuses any change to them ("interface managed by
    /// CAPsMAN", 7.23.7); they also stay while the CAP doesn't reach CAPsMAN, which the
    /// API doesn't tell apart.
    pub fn managed_radios(&self) -> HashMap<AsciiString, AsciiString> {
        if !self.wireless_cap.enabled {
            return HashMap::new();
        }
        self.wireless
            .iter()
            .filter(|radio| self.wireless_cap.interfaces.contains(&radio.data.name))
            .filter_map(|radio| Some((radio.default_name.clone()?, radio.data.name.clone())))
            .collect()
    }
}

impl WirelessDeviceDataTarget {
    pub(in crate::device::ros) fn new(model: &[u8]) -> Self {
        Self {
            wireless_cap: Default::default(),
            wireless: build_wireless_ports(model)
                .into_iter()
                .map(|port| (port.default_name, port.data))
                .collect(),
        }
    }
    /// Configures every legacy wireless radio as NetBox describes it. One with an RF
    /// role is an end of a point-to-point bridge: it gets the board's whole radio
    /// profile (`hw_facts::wireless_bridge_port`), the AP side running plain Nv2, the
    /// station side also falling back to Nstreme/802.11 (as the production links
    /// do), with the wireless link's SSID, key and regulatory settings. One without
    /// is not managed as a radio and keeps its `current` configuration (e.g. a CAP
    /// of CAPsMAN), only named after its interface if the L2 setup uses it.
    pub(in crate::device::ros) fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WirelessDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let profiles = self.wireless.clone();
        // a radio the board doesn't have is a card in its miniPCIe slot (`wlan2` of an RB912)
        for port in current.iter().flat_map(|current| current.wireless.iter()) {
            self.wireless
                .insert(port.default_name.clone(), port.data.clone());
        }
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::Wlan(_)) = interface.external_port() else {
                continue;
            };
            let key = Some(AsciiString::from(port.to_string()));
            let wlan = self
                .wireless
                .get_mut(&key)
                .ok_or(SetupError::PortNotFound(port))?;
            if let Some(radio) = interface.radio() {
                let profile = profiles
                    .get(&key)
                    .filter(|profile| profile.antenna_gain.is_some())
                    .ok_or(SetupError::NoRadioProfile(port))?;
                *wlan = profile.clone();
                configure_bridge_radio(wlan, device, port, radio)?;
            }
            if let Some((name, _)) = l2.placement_of_port(port) {
                wlan.name = name.clone();
            }
            let mtu = port_mtu(&interface)?;
            let l2mtu = profiles.get(&key).and_then(|p| p.l_2_mtu).or(wlan.l_2_mtu);
            wlan.mtu = Some(mtu.unwrap_or(DEFAULT_MTU));
            wlan.l_2_mtu = l2mtu_for(l2mtu, mtu);
        }
        self.setup_legacy_cap(device, l2, current)
    }
    /// Gives the radios CAPsMAN manages (`WirelessDeviceDataCurrent::managed_radios`) and NetBox
    /// has (`managed`, by default name) back what they have, like `/interface/wifi` its bound
    /// ones. The L2 setup already has their names (`l2::KeptNames`), and so has the CAP. That they differ from
    /// NetBox is no warning: CAPsMAN configures them, as it is meant to.
    pub fn keep_managed_radios(
        &mut self,
        current: &WirelessDeviceDataCurrent,
        managed: &HashSet<AsciiString>,
    ) {
        for radio in current.wireless.iter() {
            let Some(default_name) = &radio.default_name else {
                continue;
            };
            if !managed.contains(default_name) {
                continue;
            }
            let key = Some(default_name.clone());
            self.wireless.insert(key, radio.data.clone());
        }
    }
    /// A CAP of the legacy CAPsMAN (`Topology::is_legacy_cap`) with every radio NetBox has,
    /// finding the controller at its loopback like a CAP of `/interface/wifi`. The CAP
    /// forwards the clients' traffic itself (`local-forwarding`, `CapsManDeviceDataTarget`)
    /// into `bridge`, the one carrying the WLANs' VLANs and their VXLAN, like a CAP of
    /// `/interface/wifi`. An AP whose radios NetBox doesn't have keeps its CAP as it is (one
    /// of a CAPsMAN NetBox doesn't know of), any other device has it off.
    fn setup_legacy_cap<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WirelessDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let Some(group) = device.legacy_cap_of() else {
            self.wireless_cap = match (device.wlan_ap_of(), current) {
                (Some(_), Some(current)) => current.wireless_cap.clone(),
                _ => Default::default(),
            };
            return Ok(());
        };
        let controller = group
            .controller()
            .and_then(|controller| controller.loopback_ip())
            .and_then(|ip| ip.addr())
            .ok_or_else(|| SetupError::MissingController(device.name().into()))?;
        let cap = &mut self.wireless_cap;
        *cap = Default::default();
        cap.enabled = true;
        cap.caps_man_addresses = [controller].into();
        cap.bridge = group
            .wlan()
            .find_map(|wlan| l2.bridge_of_wlan(&wlan))
            .map_or(HasNone::NoneValue, |bridge| HasNone::Value(bridge.clone()));
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::Wlan(_)) = interface.external_port() else {
                continue;
            };
            let default_name = AsciiString::from(port.to_string());
            // a radio L2 doesn't place keeps the name it has (`generate_from` took it from the
            // device): names and default names may be crossed (bachmann-keller-ap's `wlan1`
            // is `wlan2`), so the CAP never swaps one for the other
            let name = match l2.placement_of_port(port) {
                Some((name, _)) => name.clone(),
                None => self
                    .wireless
                    .get(&Some(default_name.clone()))
                    .map_or(default_name, |wlan| wlan.name.clone()),
            };
            cap.interfaces.insert(name);
            // `wireless` installs its radios disabled (a wAP ac coming from `wifi-qcom-ac`)
            if let Some(wlan) = self.wireless.get_mut(&Some(AsciiString::from(port.to_string()))) {
                wlan.disabled = false;
            }
        }
        Ok(())
    }
}

/// Sets `wlan`, a 5 GHz bridge radio profile, up as the `radio` end of its link.
fn configure_bridge_radio(
    wlan: &mut InterfaceWirelessCfg,
    device: &DeviceAccess,
    port: PhysicalPortId,
    radio: &Radio,
) -> Result<(), SetupError> {
    let link = radio
        .link
        .as_ref()
        .ok_or(SetupError::MissingWirelessLink(port))?;
    let (mode, protocol) = match radio.role {
        RadioRole::Ap => (InterfaceWirelessMode::Bridge, Nv2),
        RadioRole::Station => (InterfaceWirelessMode::StationBridge, Nv2Nstreme80211),
    };
    wlan.mode = Some(mode);
    wlan.wireless_protocol = Some(protocol);
    // a station scans for its AP
    if radio.role == RadioRole::Ap {
        wlan.frequency = Some(radio.frequency.map_or(Auto::Auto, Auto::Value));
    }
    if let Some(width) = radio.channel_width {
        wlan.channel_width = Some(wireless_channel_width(width, radio.role)?);
    }
    wlan.ssid = Some(link.ssid.as_ref().into());
    if let Some(key) = link_key(link)? {
        wlan.nv_2_security = Some(EnabledDisabled::Enabled);
        wlan.nv_2_preshared_key = Some(key.into());
    }
    wlan.country = Some(match &link.country {
        Some(country) => country.as_ref().into(),
        None => country_settings(device)?.0.into(),
    });
    if let Some(mode) = &link.frequency_mode {
        wlan.frequency_mode = Some(
            match InterfaceWirelessFrequencyMode::parse_ros(mode.as_bytes()) {
                ParseRosValueResult::Value(mode) => mode,
                _ => return Err(SetupError::InvalidFrequencyMode(mode.clone())),
            },
        );
    }
    if let Some(power) = radio.tx_power {
        wlan.tx_power_mode = Some(InterfaceWirelessTxPowerMode::AllRatesFixed);
        wlan.tx_power = Some(power);
    }
    Ok(())
}


/// The channel width for `width` MHz with the control channel first, as on the AP
/// side; a station follows the AP's layout (`X`), whatever it is.
fn wireless_channel_width(
    width: u32,
    role: RadioRole,
) -> Result<InterfaceWirelessChannelWidth, SetupError> {
    use InterfaceWirelessChannelWidth::*;
    Ok(match (width, role) {
        (5, _) => _5Mhz,
        (10, _) => _10Mhz,
        (20, _) => _20Mhz,
        (40, RadioRole::Ap) => _2040MhzCe,
        (40, RadioRole::Station) => _2040MhzXx,
        (80, RadioRole::Ap) => _204080MhzCeee,
        (80, RadioRole::Station) => _204080MhzXxxx,
        (160, RadioRole::Ap) => _204080160MhzCeeeeeee,
        (160, RadioRole::Station) => _204080160MhzXxxxxxxx,
        _ => return Err(SetupError::UnsupportedChannelWidth(width)),
    })
}
