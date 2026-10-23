//! The 60 GHz radios (`/interface/w60g`) of a bridge link.

use super::{country_settings, link_key};
use crate::{
    device::ros::{
        SetupError, W60GDeviceDataCurrent, W60GDeviceDataTarget,
        hw_facts::build_w60g_ports,
        l2::{L2Setup, NameGenerator},
    },
    topology::{PhysicalPortId, RadioRole, access::device::DeviceAccess},
};
use mikrotik_model::{
    ascii::AsciiString,
    model::{InterfaceW60GMode, InterfaceW60GRegion},
    value::{Auto, ParseRosValueResult, RosValue},
};

impl W60GDeviceDataTarget {
    pub(in crate::device::ros) fn new(model: &[u8]) -> Self {
        Self {
            radio: build_w60g_ports(model)
                .into_iter()
                .map(|port| (port.0.name.clone(), port))
                .collect(),
        }
    }
    /// Like the legacy wireless radios: one with an RF role is a bridge end and gets
    /// the whole 60 GHz profile (`hw_facts::w60g_port`); its stations - the far ends,
    /// as dynamic `/interface/w60g/station` interfaces on the AP side - join the
    /// bridge the radio is in (`put-stations-in-bridge`), the wireless link's PSK is
    /// the password. One without keeps its `current` configuration.
    pub(in crate::device::ros) fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&W60GDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let profiles = self.radio.clone();
        for port in current.iter().flat_map(|current| current.radio.iter()) {
            if let Some(target) = self.radio.get_mut(&port.0.name) {
                *target = port.clone();
            }
        }
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::W60g(_)) = interface.external_port() else {
                continue;
            };
            let key = AsciiString::from(port.to_string());
            let radio_port = &mut self
                .radio
                .get_mut(&key)
                .ok_or(SetupError::PortNotFound(port))?
                .0;
            let placement = l2.placement_of_port(port);
            if let Some(radio) = interface.radio() {
                let link = radio
                    .link
                    .as_ref()
                    .ok_or(SetupError::MissingWirelessLink(port))?;
                *radio_port = profiles[&key].0.clone();
                radio_port.mode = Some(match radio.role {
                    RadioRole::Ap => InterfaceW60GMode::Bridge,
                    RadioRole::Station => InterfaceW60GMode::StationBridge,
                });
                if radio.role == RadioRole::Ap {
                    radio_port.frequency = Some(radio.frequency.map_or(Auto::Auto, Auto::Value));
                    radio_port.isolate_stations = Some(true);
                }
                radio_port.put_stations_in_bridge =
                    placement.and_then(|(_, bridge)| bridge.cloned());
                radio_port.ssid = Some(link.ssid.as_ref().into());
                radio_port.password = link_key(link)?.map(Into::into);
                radio_port.region = Some(match &link.country {
                    Some(region) => match InterfaceW60GRegion::parse_ros(region.as_bytes()) {
                        ParseRosValueResult::Value(region) => region,
                        _ => return Err(SetupError::UnknownCountry(region.clone())),
                    },
                    None => country_settings(device)?.1,
                });
            }
        }
        Ok(())
    }
}
