//! The controller of the legacy CAPsMAN (`/caps-man`).

use super::{country_settings, wlan_configuration_name};
use crate::{
    device::ros::{CapsManDeviceDataTarget, SetupError},
    topology::{
        WlanAuth,
        access::{device::DeviceAccess, wlan_group::WlanGroupAccess},
    },
};
use mikrotik_model::{
    model::{
        CapsAuthenticationTypes, CapsEncryption, CapsManConfigurationMode,
        CapsManDatapathVlanMode, CapsManProvisioningAction, CapsManProvisioningCfg,
        CapsManProvisioningNameFormat, WifiCountryLower as CapsManWifiCountry,
    },
    value::{HasNone, ParseRosValueResult, RosValue},
};

impl CapsManDeviceDataTarget {
    pub(in crate::device::ros) fn new(_model: &[u8]) -> Self {
        Self {
            manager: Default::default(),
            security: Default::default(),
            datapath: Default::default(),
            configuration: Default::default(),
            provisioning: Default::default(),
        }
    }
    /// The legacy CAPsMAN (`/caps-man`, in `wireless`) of a controller whose Wireless LAN
    /// Group has CAPs of it (`Topology::is_legacy_cap`), next to the `/interface/wifi` one:
    /// a configuration per WLAN (named like that one's, SSID, country, WPA2 PSK), the first
    /// WLAN the master of every radio, the others virtual ones. The CAPs forward the clients'
    /// traffic themselves, into their bridge (`WirelessDeviceDataTarget::setup_legacy_cap`)
    /// in the WLAN's VLAN, from where the VXLAN carries it on. Only read where NetBox asks
    /// for it (`DeviceCfg::fetch`).
    pub(in crate::device::ros) fn generate_from(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        let Some(group) = device
            .wlan_controller_of()
            .filter(WlanGroupAccess::has_legacy_caps)
        else {
            return Ok(());
        };
        self.manager.enabled = true;
        let country = match CapsManWifiCountry::parse_ros(country_settings(device)?.0.as_bytes()) {
            ParseRosValueResult::Value(country) => country,
            _ => return Err(SetupError::UnknownCountry(country_settings(device)?.0.into())),
        };
        let mut names = Vec::new();
        for wlan in group.wlan() {
            let ssid = wlan.ssid().unwrap_or_default();
            let name = wlan_configuration_name(ssid);
            let configuration = &mut self.configuration.entry(name.clone()).or_default().0;
            configuration.name = name.clone();
            configuration.mode = Some(CapsManConfigurationMode::Ap);
            configuration.ssid = Some(ssid.into());
            configuration.country = Some(country);
            configuration.datapath = Some(name.clone());
            if let Some(WlanAuth::Wpa(settings)) = wlan.auth() {
                let security = &mut self.security.entry(name.clone()).or_default().0;
                security.name = name.clone();
                security.authentication_types = [CapsAuthenticationTypes::Wpa2Psk].into();
                security.encryption = [CapsEncryption::AesCcm].into();
                security.passphrase = Some(settings.psk().into());
                configuration.security = Some(name.clone());
            }
            let datapath = &mut self.datapath.entry(name.clone()).or_default().0;
            datapath.name = name.clone();
            datapath.local_forwarding = Some(true);
            datapath.client_to_client_forwarding = Some(true);
            if let Some(vlan) = wlan.vlan().and_then(|vlan| vlan.vlan_id()) {
                datapath.vlan_mode = Some(CapsManDatapathVlanMode::UseTag);
                datapath.vlan_id = Some(vlan);
            }
            names.push(name);
        }
        if let Some((master, slaves)) = names.split_first() {
            let provisioning = CapsManProvisioningCfg {
                action: HasNone::Value(CapsManProvisioningAction::CreateDynamicEnabled),
                master_configuration: master.clone(),
                slave_configurations: slaves.iter().cloned().collect(),
                // `cap-<identity>`, to see where an interface belongs (no radio in the name:
                // the legacy CAPsMAN only knows these formats)
                name_format: CapsManProvisioningNameFormat::PrefixIdentity,
                name_prefix: "cap".into(),
                ..Default::default()
            };
            self.provisioning.insert(
                (
                    provisioning.radio_mac.clone(),
                    provisioning.identity_regexp.clone(),
                    provisioning.common_name_regexp.clone(),
                    provisioning.ip_address_ranges.clone(),
                    provisioning.hw_supported_modes.clone(),
                ),
                provisioning,
            );
        }
        Ok(())
    }
}
