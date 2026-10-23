//! The `/interface/wifi` radios: CAPs and the controller of a Wireless LAN Group.

use super::{WlanVlans, wifi_country, wlan_configuration_name, wlan_vlans};
use crate::{
    device::ros::{
        SetupError, WifiDeviceDataCurrent, WifiDeviceDataTarget,
        hw_facts::RadioPackage,
        l2::{L2Setup, NameGenerator},
        warning::{ApplyWarning, RadioBound},
    },
    topology::{PhysicalPortId, WlanAuth, access::device::DeviceAccess},
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        InterfaceWifiCapsmanUpgradePolicy, InterfaceWifiProvisioningAction,
        WifiAuthenticationTypes, WifiManagement, YesNo,
    },
    value::HasNone,
};

impl WifiDeviceDataTarget {
    pub(in crate::device::ros) fn new(_model: &[u8]) -> Self {
        Self {
            provisioning: Default::default(),
            datapath: Default::default(),
            cap: Default::default(),
            capsman: Default::default(),
            wifi: Default::default(),
            configuration: Default::default(),
            certificate: Default::default(),
        }
    }
    /// The native WiFi stack (`/interface/wifi`) as a Wireless LAN Group in NetBox describes
    /// it. Its controller runs CAPsMAN with a configuration per WLAN (SSID, country of the
    /// controller's site, WPA2/WPA3 PSK or OWE); the first WLAN is the master interface of
    /// every radio, the others virtual ones. An AP of the group is a CAP that finds the
    /// controller at its loopback (routed, no L2 to the controller needed) and forwards
    /// locally: a radio NetBox puts into a bridge gets a datapath to that bridge, which
    /// RouterOS adds the radio and its virtual interfaces to (so `setup_l2` leaves out its
    /// bridge port). A radio without a bridge of its own goes into the VLAN of the group's
    /// WLANs, on the bridge carrying it (`bridge-caps`, with the VXLAN to the controller):
    /// through a datapath with that `vlan-id` where the radio package tags
    /// (`RadioPackage::tags_vlans`), else as an access port of the bridge (`setup_l2`). The
    /// CAP sets the VLAN, not the controller, which doesn't know its CAPs' boards; so a radio
    /// has one VLAN, and WLANs of a group with different VLANs are refused. Radios of a
    /// device that is no CAP keep their current configuration. Certificates stay, except
    /// the ones CAPsMAN generated (`capsman_certificates`) on a device that is neither
    /// controller nor CAP anymore.
    pub(in crate::device::ros) fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WifiDeviceDataCurrent>,
        capsman_certificates: &[AsciiString],
        radio_package: Option<RadioPackage>,
        bound_radios: &[AsciiString],
    ) -> Result<Vec<ApplyWarning>, SetupError> {
        let mut warnings = Vec::new();
        // without a package of `/interface/wifi`, its radios are what one left (a wAP ac on
        // `wireless` now): inactive, but keeping their names, which the `wlan<N>` need
        let drivers = radio_package.is_some_and(RadioPackage::is_wifi);
        for port in current.iter().flat_map(|current| current.wifi.iter()) {
            let mut data = port.data.clone();
            if let (false, Some(default_name)) = (drivers, &port.default_name) {
                data.name = default_name.clone();
            }
            self.wifi.insert(port.default_name.clone(), data);
        }
        let in_wlan = device.wifi_cap_of().is_some() || device.wlan_controller_of().is_some();
        for certificate in current.iter().flat_map(|current| current.certificate.iter()) {
            if in_wlan || !capsman_certificates.contains(&certificate.0.name) {
                self.certificate
                    .insert(certificate.0.name.clone(), certificate.clone());
            }
        }
        if let Some(group) = device.wifi_cap_of() {
            let controller = group
                .controller()
                .and_then(|controller| controller.loopback_ip())
                .and_then(|ip| ip.addr())
                .ok_or_else(|| SetupError::MissingController(device.name().into()))?;
            let cap = &mut self.cap;
            cap.enabled = YesNo::Yes;
            cap.caps_man_addresses = [controller].into();
            let wlan_vlans = wlan_vlans(&group);
            let wlan_bridge = group.wlan().find_map(|wlan| l2.bridge_of_wlan(&wlan));
            let tags = radio_package.is_some_and(RadioPackage::tags_vlans);
            // an AP without radios in NetBox (a controller also marked as AP) has none to tag
            let has_radios = device
                .interfaces()
                .iter()
                .any(|interface| matches!(interface.external_port(), Some(PhysicalPortId::Wifi(_))));
            if wlan_vlans == WlanVlans::PerWlan && !tags && has_radios {
                return Err(SetupError::WlanVlansNotTagged(device.name().into()));
            }
            // a configuration per datapath (`None`: none, an access port), the first `capsman`
            let mut cap_configurations: Vec<(Option<AsciiString>, AsciiString)> = Vec::new();
            for interface in device.interfaces() {
                let Some(port @ PhysicalPortId::Wifi(_)) = interface.external_port() else {
                    continue;
                };
                let default_name = AsciiString::from(port.to_string());
                let radio = self
                    .wifi
                    .get_mut(&Some(default_name.clone()))
                    .ok_or(SetupError::PortNotFound(port))?;
                let placement = l2.placement_of_port(port);
                // with a VLAN per WLAN, the controller's configurations have them
                let vlan = match (placement.and_then(|(_, bridge)| bridge), wlan_bridge, wlan_vlans) {
                    (None, Some(_), WlanVlans::One(Some(vlan))) if tags => Some(vlan),
                    _ => None,
                };
                let bridge = match placement {
                    None => None,
                    Some((_, Some(bridge))) => Some(bridge),
                    // an access port of the bridge, which `setup_l2` adds
                    Some((_, None)) if !tags => None,
                    Some((_, None)) => wlan_bridge,
                };
                if let Some(bridge) = bridge {
                    let datapath = &mut self.datapath.entry(bridge.clone()).or_default().0;
                    datapath.name = bridge.clone();
                    datapath.bridge = Some(HasNone::Value(bridge.clone()));
                    datapath.vlan_id = vlan.map(HasNone::Value);
                    datapath.disabled = false;
                    // virtual interfaces of the CAP use the one of the first radio
                    self.cap.slaves_datapath.get_or_insert_with(|| bridge.clone());
                }
                let configuration_name = match cap_configurations
                    .iter()
                    .find(|(datapath, _)| datapath.as_ref() == bridge)
                {
                    Some((_, name)) => name.clone(),
                    None => {
                        let name = match (cap_configurations.is_empty(), bridge) {
                            (true, _) => AsciiString::from("capsman"),
                            (false, Some(bridge)) => format!("capsman-{bridge}").as_str().into(),
                            (false, None) => AsciiString::from("capsman-local"),
                        };
                        cap_configurations.push((bridge.cloned(), name.clone()));
                        let configuration =
                            &mut self.configuration.entry(name.clone()).or_default().0;
                        configuration.name = name.clone();
                        configuration.disabled = false;
                        configuration.manager = Some(WifiManagement::Capsman);
                        configuration.datapath = bridge.cloned();
                        // reported with the datapath's values, which are set to the same
                        configuration.datapath_bridge = bridge.cloned();
                        configuration.datapath_vlan_id = vlan.map(HasNone::Value);
                        name
                    }
                };
                // A radio CAPsMAN has bound is dynamic and RouterOS refuses any change to it
                // (7.23.8, also with the CAP off): it stays as it is, a different configuration
                // is a warning.
                if bound_radios.contains(&default_name) {
                    if radio.configuration.as_ref() != Some(&configuration_name) {
                        warnings.push(ApplyWarning::RadioBound(RadioBound {
                            radio: default_name.to_string().into(),
                            configuration: radio.configuration.as_ref().map(|c| c.to_string().into()),
                            expected: configuration_name.to_string().into(),
                        }));
                    }
                    continue;
                }
                // under the name L2 gives it; the configuration has it all, which the radio
                // reports as its own
                radio.name = default_name;
                radio.configuration = Some(configuration_name);
                radio.configuration_manager = Some(WifiManagement::Capsman);
                radio.datapath = None;
                radio.datapath_bridge = bridge.cloned();
                radio.datapath_vlan_id = vlan.map(HasNone::Value);
            }
        }
        // what a bound radio uses stays, and with it its datapath
        for radio in current.iter().flat_map(|current| current.wifi.iter()) {
            let bound = radio
                .default_name
                .as_ref()
                .is_some_and(|name| bound_radios.contains(name));
            let Some(name) = radio.data.configuration.as_ref().filter(|_| bound) else {
                continue;
            };
            let Some(used) = current
                .iter()
                .flat_map(|current| current.configuration.iter())
                .find(|configuration| &configuration.0.name == name)
            else {
                continue;
            };
            let configuration = self
                .configuration
                .entry(name.clone())
                .or_insert_with(|| used.clone());
            if let Some(datapath) = configuration.0.datapath.clone()
                && let Some(used) = current
                    .iter()
                    .flat_map(|current| current.datapath.iter())
                    .find(|used| used.0.name == datapath)
            {
                self.datapath.entry(datapath).or_insert_with(|| used.clone());
            }
        }
        if let Some(group) = device.wlan_controller_of() {
            let capsman = &mut self.capsman;
            capsman.enabled = YesNo::Yes;
            // RouterOS's defaults, which it reports only once set
            capsman.interfaces = Some([HasNone::Value("all".into())].into());
            capsman.upgrade_policy = Some(InterfaceWifiCapsmanUpgradePolicy::SuggestSameVersion);
            capsman.require_peer_certificate = Some(false);
            let country = wifi_country(device)?;
            let per_wlan = wlan_vlans(&group) == WlanVlans::PerWlan;
            let mut names = Vec::new();
            for wlan in group.wlan() {
                let ssid = wlan.ssid().unwrap_or_default();
                let name = wlan_configuration_name(ssid);
                let configuration = &mut self.configuration.entry(name.clone()).or_default().0;
                configuration.name = name.clone();
                configuration.disabled = false;
                configuration.ssid = Some(ssid.into());
                configuration.country = Some(country);
                // the CAP's datapath gives the bridge, this the VLAN in it
                configuration.datapath_vlan_id = wlan
                    .vlan()
                    .and_then(|vlan| vlan.vlan_id())
                    .filter(|_| per_wlan)
                    .map(HasNone::Value);
                match wlan.auth() {
                    Some(WlanAuth::Wpa(settings)) => {
                        configuration.security_authentication_types =
                            [WifiAuthenticationTypes::Wpa2Psk, WifiAuthenticationTypes::Wpa3Psk]
                                .into();
                        configuration.security_passphrase = Some(settings.psk().into());
                    }
                    Some(WlanAuth::Open(settings)) if settings.use_owe => {
                        configuration.security_authentication_types =
                            [WifiAuthenticationTypes::Owe].into();
                    }
                    Some(WlanAuth::Open(_)) | None => {}
                }
                names.push(name);
            }
            if let Some((master, slaves)) = names.split_first() {
                let provisioning = self.provisioning.entry(Default::default()).or_default();
                provisioning.action = InterfaceWifiProvisioningAction::CreateDynamicEnabled;
                provisioning.disabled = false;
                provisioning.master_configuration = Some(master.clone());
                // the CAP and its radio in the name, to see where an interface belongs; the
                // virtual ones follow it (`<master>-virtual`)
                provisioning.name_format = Some("cap-%I-%r".into());
                provisioning.slave_configurations =
                    (!slaves.is_empty()).then(|| slaves.iter().cloned().collect());
            }
        }
        Ok(warnings)
    }
}
