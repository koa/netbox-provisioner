//! Scope: The base config's step for PoE out and its warning; not reading PoE from a device
//! (`device::poe`).
//!
//! PoE out on the ports NetBox gives `pse`, and the warning where an apply would cut it.
use super::{BaseDeviceDataCurrent, BaseDeviceDataTarget, SetupError};
use crate::{
    device::ros::warning::{ApplyWarning, PoeCut},
    topology::access::device::DeviceAccess,
};
use mikrotik_model::{ascii::AsciiString, model::InterfaceEthernetPoeOut};
use std::collections::HashSet;

impl BaseDeviceDataTarget {
    /// PoE out of every port NetBox has: `auto-on` with `poe_mode=pse`, `forced-on` if its
    /// `poe_type` is passive as well, else `off`. `auto-on` feeds only a load it detects, which
    /// a UISP Wave Nano, passive PoE only, isn't (lab-np1 `e08`: `waiting-for-load`, with
    /// `forced-on` `powered-on` at 23.5 V, 7.23.8). `forced-on` puts the voltage on the port
    /// without asking, so only where NetBox says the device at the other end takes it.
    pub(super) fn setup_poe(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        for port in device.interfaces() {
            if let Some(port_id) = port.external_port()
                && let Some(ethernet_port) = self.get_ethernet_port(port_id)?
            {
                // `generate_ethernet()` already leaves `poe_out` at `None` for
                // boards without PoE-out hardware (e.g. CHR, several switch
                // models); only override it here if the baseline already
                // established PoE capability, otherwise RouterOS rejects the
                // whole `/interface/ethernet/set` call with "bad parameter
                // poe-out" - breaking the rename bundled into the same call.
                if ethernet_port.poe_out.is_some() {
                    ethernet_port.poe_out = Some(match (port.enable_poe(), port.passive_poe()) {
                        (true, true) => InterfaceEthernetPoeOut::ForcedOn,
                        (true, false) => InterfaceEthernetPoeOut::AutoOn,
                        (false, _) => InterfaceEthernetPoeOut::Off,
                    });
                }
            }
        }
        Ok(())
    }

    /// Every port whose PoE out is on and that this target switches off (NetBox has no
    /// `poe_mode=pse` there), by its name at the device.
    pub fn poe_switched_off(&self, current: &BaseDeviceDataCurrent) -> Vec<(AsciiString, PoeCut)> {
        let mut ports = Vec::new();
        for (default_name, target) in &self.ethernet {
            let Some(current) = current
                .ethernet
                .iter()
                .find(|current| current.default_name == *default_name)
            else {
                continue;
            };
            if target.poe_out == Some(InterfaceEthernetPoeOut::Off)
                && current
                    .data
                    .poe_out
                    .is_some_and(|poe_out| poe_out != InterfaceEthernetPoeOut::Off)
            {
                let cut = PoeCut {
                    port: target.name.to_string().into(),
                    default_name: default_name.to_string().into(),
                };
                ports.push((current.data.name.clone(), cut));
            }
        }
        ports
    }

    /// The ports of `poe_switched_off` that power something now (`powered`, by name at the
    /// device): applying the target cuts its power.
    pub fn poe_warnings(
        &self,
        current: &BaseDeviceDataCurrent,
        powered: &HashSet<AsciiString>,
    ) -> Vec<ApplyWarning> {
        self.poe_switched_off(current)
            .into_iter()
            .filter(|(name, _)| powered.contains(name))
            .map(|(_, cut)| ApplyWarning::PoeCut(cut))
            .collect()
    }
}
