//! Scope: Tests of the base config, one module per topic, and the fixtures they share; no test
//! of a topic itself.
mod addresses;
mod capsman;
mod dhcp;
mod firewall;
mod hand_made;
mod mac_server;
mod mtu;
mod netbox_example;
mod poe;
mod radio;
mod service;
mod socks;
mod speed;
mod syslog;
mod uplink;
mod watchdog;
mod wlan_vlans;

use crate::device::ros::{BaseDeviceDataCurrent, BaseDeviceDataTarget};
use mikrotik_model::{
    model::{
        InterfaceEthernetByDefaultName, InterfaceEthernetCfg, SystemLoggingActionByName,
        SystemLoggingActionRemoteLogFormat, SystemLoggingById,
    },
    value::Id,
};

/// The state of a freshly reset device with the same ports as `target`: every
/// port under its default name, nothing else configured.
pub(crate) fn empty_current(target: &BaseDeviceDataTarget) -> BaseDeviceDataCurrent {
    BaseDeviceDataCurrent {
        ospf_interface: Box::new([]),
        interface_list: Box::new([]),
        identity: Default::default(),
        bridge: Box::new([]),
        bridge_port: Box::new([]),
        ethernet: target
            .ethernet
            .iter()
            .map(|(default_name, e)| InterfaceEthernetByDefaultName {
                default_name: default_name.clone(),
                data: InterfaceEthernetCfg {
                    name: default_name.clone(),
                    ..e.clone()
                },
            })
            .collect(),
        ipv_6_address: Box::new([]),
        ospf_instance: Box::new([]),
        vxlan: Box::new([]),
        ospf_area: Box::new([]),
        vxlan_vteps: Box::new([]),
        interface_list_member: Box::new([]),
        bridge_vlan: Box::new([]),
        ipv_6_firewall_address_list: Box::new([]),
        ipv_4_address: Box::new([]),
        vlan: Box::new([]),
        dhcp_v_4_client: Box::new([]),
        dhcp_v_6_client: Box::new([]),
        ipv_6_firewall_filter: Box::new([]),
        ip_firewall_filter: Box::new([]),
        ip_firewall_nat: Box::new([]),
        dns: Default::default(),
        ipv_6_nd: Box::new([]),
        dhcp_v_4_server: Box::new([]),
        dhcp_v_4_server_network: Box::new([]),
        dhcp_v_4_lease: Box::new([]),
        ipv_4_pool: Box::new([]),
        socks: Default::default(),
        socks_users: Box::new([]),
        socks_access: Box::new([]),
        ip_service: Box::new([]),
        ssh: Default::default(),
        ipv_6_settings: Default::default(),
        mac_server: Default::default(),
        neighbor_discovery: Default::default(),
        watchdog: Default::default(),
        logging_action: routeros_logging_actions(),
        logging: crate::device::ros::syslog::routeros_logging()
            .1
            .into_values()
            .enumerate()
            .map(|(id, data)| SystemLoggingById {
                id: Id(id as u32 + 1),
                data,
            })
            .collect(),
    }
}

/// RouterOS's logging actions on a reset device: `remote` in the default format, which hides
/// the CEF fields (CHR 7.24.5).
fn routeros_logging_actions() -> Box<[SystemLoggingActionByName]> {
    let mut actions = crate::device::ros::syslog::routeros_logging().0;
    let remote = &mut actions
        .get_mut(&b"remote".as_slice().into())
        .expect("RouterOS's remote action")
        .0;
    remote.remote_log_format = Some(SystemLoggingActionRemoteLogFormat::Default);
    remote.syslog_time_format = None;
    actions.into_values().collect()
}
