use crate::topology::access::DeviceAccess;
use crate::{config::CONFIG, device::AccessibleDevice, Error};
use convert_case::{Case, Casing};
use ipnet::IpNet;
use ipnet::Ipv4Net;
use ipnet::Ipv6Net;
use mikrotik_model::{
    ascii::AsciiString, hwconfig::DeviceType, mikrotik_model, value, MikrotikDevice,
};
use std::{collections::hash_map::Entry, net::IpAddr};

mod graphql;

impl AccessibleDevice {
    pub async fn create_client(
        &self,
        target: Option<IpAddr>,
        credentials: Option<Box<str>>,
    ) -> Result<MikrotikDevice, Error> {
        let addr = target.unwrap_or(self.address);
        let credential_name = credentials.unwrap_or_else(|| self.credentials.clone());
        let key = (addr, credential_name);
        if let Some(credentials) = CONFIG.mikrotik_credentials.get(key.1.as_ref()) {
            let mut client_ref = self.clients.lock().await;
            Ok(match client_ref.entry(key.clone()) {
                Entry::Occupied(e) => e.get().clone(),
                Entry::Vacant(v) => v
                    .insert(
                        MikrotikDevice::connect(
                            (self.address, 8728),
                            credentials.user().as_bytes(),
                            credentials.password().map(|p| p.as_bytes()),
                        )
                        .await?,
                    )
                    .clone(),
            })
        } else {
            Err(Error::MissingCredentials)
        }
    }
}

mikrotik_model!(
    name = DeviceData,
    detect = new,
    fields(
        identity(single = "system/identity"),
        interface_list(by_key(path = "interface/list", key = name)),
        interface_list_member(by_id(path = "interface/list/member", keys(interface, list))),
        wireless_cap(single = "interface/wireless/cap"),
        ethernet(by_key(path = "interface/ethernet", key = defaultName)),
        wireless(by_key(path = "interface/wireless", key = defaultName)),
        bridge(by_key(path = "interface/bridge", key = name)),
        bridge_port(by_id(
            path = "interface/bridge/port",
            keys(bridge, interface)
        )),
        ipv4_address(by_key(path = "ip/address", key = address)),
        ipv6_address(by_key(path = "ipv6/address", key = address)),
        vxlan(by_key(path = "interface/vxlan", key = name)),
        vxlan_vteps(by_id(
            path = "interface/vxlan/vteps",
            keys(interface, remote_ip)
        )),
    ),
);
impl DeviceDataTarget {
    fn new(device_type: DeviceType) -> Self {
        Self {
            ethernet: device_type
                .build_ethernet_ports()
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
            identity: Default::default(),
            bridge: Default::default(),
            bridge_port: Default::default(),
            interface_list: Default::default(),
            interface_list_member: Default::default(),
            wireless_cap: Default::default(),
            wireless: device_type
                .build_wireless_ports()
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
            ipv_4_address: Default::default(),
            ipv_6_address: Default::default(),
            vxlan: Default::default(),
            vxlan_vteps: Default::default(),
        }
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
    fn generate_from(&mut self, device: &DeviceAccess) {
        if let Some(loopback_ip) = device.loopback_ip() {
            self.set_loopback_ip(loopback_ip);
        }
        self.set_fixed_addresses(device);
        if let Some(wlan_group) = device.wlan_ap_of() {
            if let (Some(vxlan), Some(my_ip)) = (wlan_group.transport_vxlan(), device.loopback_ip())
            {
                if let (Some(name), Some(vni)) = (
                    vxlan
                        .name()
                        .map(cleanup_name)
                        .map(|name| format!("vxlan-{name}").to_case(Case::Kebab))
                        .map(AsciiString::from),
                    vxlan.vni(),
                ) {
                    self.vxlan.insert(
                        name.clone(),
                        InterfaceVxlanByName(InterfaceVxlanCfg {
                            vni,
                            ..Default::default()
                        }),
                    );
                    for remote_vtep_addr in vxlan.vteps().into_iter().filter(|ip| ip != &my_ip) {
                        self.vxlan_vteps
                            .insert((name.clone(), remote_vtep_addr), Default::default());
                    }
                }
            }
        }
    }

    fn set_fixed_addresses(&mut self, device: &DeviceAccess) {
        for interface in device.interfaces() {
            if let Some(port) = interface
                .external_port()
                .map(|p| AsciiString::from(p.to_string()))
            {
                if self.ethernet.contains_key(&port) {
                    for ipnet in interface.ips() {
                        match ipnet {
                            IpNet::V4(ip_net) => {
                                self.ipv_4_address.insert(
                                    *ip_net,
                                    IpAddressByAddress(IpAddressCfg {
                                        interface: port.clone(),
                                        ..Default::default()
                                    }),
                                );
                            }
                            IpNet::V6(ip_net) => {
                                self.ipv_6_address.insert(
                                    *ip_net,
                                    Ipv6AddressByAddress(Ipv6AddressCfg {
                                        interface: port.clone(),
                                        ..Default::default()
                                    }),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn set_loopback_ip(&mut self, loopback_ip: IpAddr) {
        match loopback_ip {
            IpAddr::V4(loopback_ip) => {
                self.ipv_4_address.insert(
                    loopback_ip.into(),
                    IpAddressByAddress(IpAddressCfg {
                        interface: b"lo".into(),
                        ..IpAddressCfg::default()
                    }),
                );
            }
            IpAddr::V6(loopback_ip) => {
                self.ipv_6_address.insert(
                    loopback_ip.into(),
                    Ipv6AddressByAddress(Ipv6AddressCfg {
                        interface: b"lo".into(),
                        ..Ipv6AddressCfg::default()
                    }),
                );
            }
        }
    }
}
fn cleanup_name(name: &str) -> String {
    name.replace(['.', '/', '+', ':'], "_")
}
