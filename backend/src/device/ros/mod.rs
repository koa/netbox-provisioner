use crate::{
    Error,
    config::CONFIG,
    device::{AccessibleDevice, Credentials},
    topology::access::DeviceAccess,
};
use convert_case::{Case, Casing};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    MikrotikDevice,
    ascii::{self, AsciiString},
    hwconfig::DeviceType,
    mikrotik_model,
    model::{
        InterfaceVxlanByName, InterfaceVxlanCfg, IpAddressByAddress, IpAddressCfg,
        Ipv6AddressByAddress, Ipv6AddressCfg,
    },
    value,
};
use std::net::IpAddr;

mod graphql;

impl AccessibleDevice {
    pub async fn create_client(
        &self,
        target: Option<IpAddr>,
        credentials: Credentials,
    ) -> Result<MikrotikDevice, Error> {
        let addr = target.unwrap_or(self.address);
        let key = (addr, credentials.clone());
        let mut client_ref = self.clients.lock().await;
        Ok(if let Some(client) = client_ref.get(&key) {
            client.clone()
        } else {
            let (username, password) = match &credentials {
                Credentials::Default => {
                    let c = CONFIG
                        .mikrotik_credentials
                        .get(&self.credentials)
                        .ok_or(Error::MissingCredentials)?;
                    (c.user(), c.password())
                }
                Credentials::Named(name) => {
                    let c = CONFIG
                        .mikrotik_credentials
                        .get(name.as_ref())
                        .ok_or(Error::MissingCredentials)?;
                    (c.user(), c.password())
                }
                Credentials::Adhoc { username, password } => (
                    username.as_ref().map(Box::as_ref).unwrap_or("admin"),
                    password.as_ref().map(Box::as_ref),
                ),
            };
            let mikrotik_device = MikrotikDevice::connect(
                (addr, 8728),
                username.as_bytes(),
                password.map(|p| p.as_bytes()),
            )
            .await?;
            client_ref.put(key, mikrotik_device.clone());
            mikrotik_device
        })
    }
}

mikrotik_model!(
    name = BaseDeviceData,
    detect = new,
    fields(
        identity(single = "system/identity"),
        interface_list(by_key(path = "interface/list", key = name)),
        interface_list_member(by_id(path = "interface/list/member", keys(interface, list))),
        ethernet(by_key(path = "interface/ethernet", key = defaultName)),
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
impl BaseDeviceDataTarget {
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
        self.set_identity(device.name());
        if let Some(loopback_ip) = device.loopback_ip() {
            self.set_loopback_ip(loopback_ip);
        }
        self.set_fixed_addresses(device);
        if let Some(wlan_group) = device.wlan_ap_of() {
            if let (Some(vxlan), Some(my_ip)) =
                (wlan_group.transport_vxlan(), device.primary_ip_v4())
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
mikrotik_model!(
    name = WirelessDeviceData,
    detect = new,
    fields(
        wireless_cap(single = "interface/wireless/cap"),
        wireless(by_key(path = "interface/wireless", key = defaultName)),
    ),
);

impl WirelessDeviceDataTarget {
    fn new(device_type: DeviceType) -> Self {
        Self {
            wireless_cap: Default::default(),
            wireless: device_type
                .build_wireless_ports()
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
        }
    }
    fn generate_from(&mut self, device: &DeviceAccess) {}
}
mikrotik_model!(
    name = WifiDeviceData,
    detect = new,
    fields(
        cap(single = "interface/wifi/cap"),
        datapath(by_key(path = "interface/wifi/datapath", key = name)),
    ),
);
impl WifiDeviceDataTarget {
    fn new(device_type: DeviceType) -> Self {
        Self {
            datapath: Default::default(),
            cap: Default::default(),
        }
    }
}
fn cleanup_name(name: &str) -> String {
    name.replace(['.', '/', '+', ':'], "_")
}
