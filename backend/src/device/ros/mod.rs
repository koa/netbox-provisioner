use crate::{
    Error,
    config::CONFIG,
    device::{
        AccessibleDevice, Credentials,
        ros::hw_facts::{build_ethernet_ports, build_wireless_ports},
    },
    topology::access::{DeviceAccess, InterfaceAccess},
};
use convert_case::{Case, Casing};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use log::error;
use mikrotik_model::{
    MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        InterfaceVxlanByName, InterfaceVxlanCfg, IpAddressByAddress, IpAddressCfg,
        Ipv6AddressByAddress, Ipv6AddressCfg, RoutingOspfInstanceByName, RoutingOspfInstanceCfg,
        RoutingOspfInstanceVersion, RoutingRedistribute,
    },
    value,
};
use std::{collections::BTreeSet, net::IpAddr};

mod graphql;
mod hw_facts;

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
        ospf_instance(by_key(path = "routing/ospf/instance", key = name)),
        ospf_area(by_key(path = "routing/ospf/area", key = name)),
        ospf_interface(by_id(path = "routing/ospf/interface-template", keys(area))),
    ),
);
impl BaseDeviceDataTarget {
    fn new(model: &[u8]) -> Self {
        let ethernet_ports = build_ethernet_ports(model);
        if ethernet_ports.is_empty() {
            error!("No ethernet ports found for device {}",   AsciiString::from(model));
        }
        Self {
            ethernet: ethernet_ports
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
            identity: Default::default(),
            bridge: Default::default(),
            bridge_port: Default::default(),
            ospf_area: Default::default(),
            interface_list: Default::default(),
            interface_list_member: Default::default(),
            ipv_4_address: Default::default(),
            ipv_6_address: Default::default(),
            vxlan: Default::default(),
            vxlan_vteps: Default::default(),
            ospf_instance: Default::default(),
            ospf_interface: Default::default(),
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
        self.setup_ospf(device);
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
            if let Some(port) = interface.external_port().map(|p| p) {
                if self
                    .ethernet
                    .contains_key(&AsciiString::from(port.to_string()))
                {
                    for ipnet in interface.ips() {
                        match ipnet {
                            IpNet::V4(ip_net) => {
                                self.ipv_4_address.insert(
                                    *ip_net,
                                    IpAddressByAddress(IpAddressCfg {
                                        interface: port.short_name(),
                                        ..Default::default()
                                    }),
                                );
                            }
                            IpNet::V6(ip_net) => {
                                self.ipv_6_address.insert(
                                    *ip_net,
                                    Ipv6AddressByAddress(Ipv6AddressCfg {
                                        interface: port.short_name(),
                                        ..Default::default()
                                    }),
                                );
                            }
                        }
                    }
                } else {
                    error!("Port not defined {}", port);
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

    fn setup_ospf(&mut self, device: &DeviceAccess) {
        if let Some(router_id) = device.primary_ip_v4() {
            let ports = device
                .interfaces()
                .into_iter()
                .filter(InterfaceAccess::use_ospf)
                .filter_map(|i| InterfaceAccess::external_port(&i))
                .map(|p| p.short_name())
                .collect::<BTreeSet<_>>();
            if !ports.is_empty() {
                let v2_instance = self
                    .ospf_instance
                    .entry(b"default-v2".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v2_instance.0.redistribute =
                    [RoutingRedistribute::Connected, RoutingRedistribute::Static]
                        .into_iter()
                        .collect();
                v2_instance.0.router_id = Some(router_id);
                v2_instance.0.version = RoutingOspfInstanceVersion::_2;
                let v3_instance = self
                    .ospf_instance
                    .entry(b"default-v3".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v3_instance.0.redistribute =
                    [RoutingRedistribute::Connected, RoutingRedistribute::Static]
                        .into_iter()
                        .collect();
                v3_instance.0.router_id = Some(router_id);
                v3_instance.0.version = RoutingOspfInstanceVersion::_3;
                let v2_area = self.ospf_area.entry(b"backbone-v2".into()).or_default();
                v2_area.0.instance = b"default-v2".into();
                let v3_area = self.ospf_area.entry(b"backbone-v3".into()).or_default();
                v3_area.0.instance = b"default-v3".into();
                let v2_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v2".into(),))
                    .or_default();
                v2_backbone.interfaces = ports.clone();
                let v3_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v3".into(),))
                    .or_default();
                v3_backbone.interfaces = ports.clone();
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
    fn new(model: &[u8]) -> Self {
        Self {
            wireless_cap: Default::default(),
            wireless: build_wireless_ports(model)
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
    fn new(model: &[u8]) -> Self {
        Self {
            datapath: Default::default(),
            cap: Default::default(),
        }
    }
}
fn cleanup_name(name: &str) -> String {
    name.replace(['.', '/', '+', ':'], "_")
}
