//! Scope: The base config's step for the maintenance list: mac-telnet and neighbor discovery on
//! the ports to own devices and the free ones; not the list's settings (`BaseDeviceDataTarget`'s
//! frame).
use super::{
    BaseDeviceDataTarget, MAINTENANCE_LIST,
    l2::{L2Setup, NameGenerator},
};
use crate::topology::access::device::DeviceAccess;
use mikrotik_model::model::InterfaceListMemberCfg;
use std::collections::{BTreeSet, HashSet};

impl BaseDeviceDataTarget {
    /// mac-telnet for remote maintenance, on the ports to own devices and the free ones:
    /// - ports cabled to a device with RouterOS in NetBox, and radios of a wireless link: a
    ///   neighbor reaching a device that lost its addresses or routes
    ///   (`docs/update-fallback.md`);
    /// - Ethernet ports NetBox doesn't use (not there, or without cable, address, VLAN and
    ///   bridge): a notebook plugged into a free port on site.
    ///
    /// Customer ports stay out, also those in the same bridge: the list takes the port itself,
    /// which RouterOS honors for a bridge port too (CHR 7.24.5). Runs after `setup_l2`, which
    /// names the ports.
    pub(super) fn setup_maintenance_ports<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
    ) {
        let mut members = BTreeSet::new();
        let mut used = HashSet::new();
        for port in device.interfaces() {
            let Some(id) = port.external_port() else {
                continue;
            };
            let unused = port.cable().is_none()
                && port.ips().is_empty()
                && port.untagged_vlan().is_none()
                && port.tagged_vlans().next().is_none()
                && port.bridge().is_none();
            if !unused && let Some(default_name) = id.default_name() {
                used.insert(default_name);
            }
            let own = port
                .connected_interfaces()
                .iter()
                .any(|far| far.device().is_some_and(|d| d.has_routeros()))
                || port.radio().is_some_and(|radio| radio.link.is_some());
            if own && let Some(placement) = l2.placement_of_port(id) {
                members.insert(placement.name.clone());
            }
        }
        for (default_name, ethernet) in &self.ethernet {
            if !used.contains(default_name) {
                members.insert(ethernet.name.clone());
            }
        }
        for name in members {
            self.interface_list_member.insert(
                (name.clone(), MAINTENANCE_LIST.into()),
                InterfaceListMemberCfg {
                    comment: None,
                    disabled: false,
                    interface: name,
                    list: MAINTENANCE_LIST.into(),
                },
            );
        }
    }
}
