use crate::{
    Error,
    config::CONFIG,
    device::{AccessibleDevice, Credentials, ros::hw_facts::build_ethernet_ports},
    topology::access::VxlanAccess,
    topology::{
        PhysicalPortId,
        access::{DeviceAccess, InterfaceAccess},
    },
};
use convert_case::{Case, Casing};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use log::{error, info};
use mikrotik_model::{
    MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        InterfaceBridgeProtocolMode, InterfaceVxlanByName, InterfaceVxlanCfg, IpAddressByAddress,
        IpAddressCfg, Ipv6AddressByAddress, Ipv6AddressCfg, RoutingOspfInstanceByName,
        RoutingOspfInstanceCfg, RoutingOspfInstanceVersion, RoutingRedistribute,
    },
    value,
    value::PossibleRangeDash,
};
use std::{
    collections::{BTreeSet, HashSet},
    net::{IpAddr, Ipv4Addr},
};

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
        bridge_vlan(by_id(
            path = "interface/bridge/vlan",
            keys(bridge, tagged, vlan_ids)
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

const CAPS_BRIDGE_NAME: &'static [u8; 11] = b"bridge-caps";

impl BaseDeviceDataTarget {
    fn new(model: &[u8]) -> Self {
        let ethernet_ports = build_ethernet_ports(model);
        if ethernet_ports.is_empty() {
            error!(
                "No ethernet ports found for device {}",
                AsciiString::from(model)
            );
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
            bridge_vlan: Default::default(),
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
        self.setup_wlan_ap(device);
    }

    fn setup_wlan_ap(&mut self, device: &DeviceAccess) {
        if let Some(wlan_group) = device.wlan_ap_of() {
            let bridge_caps = self.bridge.entry(CAPS_BRIDGE_NAME.into()).or_default();
            bridge_caps.0.vlan_filtering = true;
            bridge_caps.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
            if let Some(my_ip) = device.primary_ip_v4() {
                let mut vlans = HashSet::new();
                if let Some(mgmt_vlan) = wlan_group.mgmt_vlan() {
                    vlans.insert(mgmt_vlan);
                }
                for wlan in wlan_group.wlan() {
                    if let Some(vlan) = wlan.vlan() {
                        vlans.insert(vlan);
                    }
                }
                let vxlans = vlans
                    .iter()
                    .filter_map(|vlan| vlan.vxlan())
                    .collect::<HashSet<_>>();
                for vxlan in vxlans {
                    self.setup_vxlan(vxlan, &my_ip);
                }
            }
        }
    }

    fn setup_vxlan(&mut self, vxlan: VxlanAccess, my_ip: &Ipv4Addr) {
        if let (Some(name), Some(vni)) = (
            vxlan
                .name()
                .map(cleanup_name)
                .map(|name| format!("vxlan-{name}").to_case(Case::Kebab))
                .map(AsciiString::from),
            vxlan.vni(),
        ) {
            self.bridge_port
                .entry((CAPS_BRIDGE_NAME.into(), name.clone()))
                .or_default();
            self.bridge_vlan
                .entry((
                    CAPS_BRIDGE_NAME.into(),
                    BTreeSet::from([name.clone()]),
                    BTreeSet::from([PossibleRangeDash::Range {
                        start: 1,
                        end: 4094,
                    }]),
                ))
                .or_default();
            self.vxlan.insert(
                name.clone(),
                InterfaceVxlanByName(InterfaceVxlanCfg {
                    vni,
                    ..Default::default()
                }),
            );
            for remote_vtep_addr in vxlan.vteps().into_iter().filter(|ip| ip != my_ip) {
                self.vxlan_vteps
                    .insert((name.clone(), remote_vtep_addr), Default::default());
            }
        }
    }

    fn set_fixed_addresses(&mut self, device: &DeviceAccess) {
        for interface in device.interfaces() {
            if let (Some(port), Some(name)) =
                (interface.external_port(), interface.interface_name())
            {
                for ip_net in interface.ips() {
                    match ip_net {
                        IpNet::V4(ip_net) => {
                            self.ipv_4_address.insert(
                                *ip_net,
                                IpAddressByAddress(IpAddressCfg {
                                    interface: name.clone(),
                                    ..Default::default()
                                }),
                            );
                        }
                        IpNet::V6(ip_net) => {
                            self.ipv_6_address.insert(
                                *ip_net,
                                Ipv6AddressByAddress(Ipv6AddressCfg {
                                    interface: name.clone(),
                                    ..Default::default()
                                }),
                            );
                        }
                    }
                }
                match &port {
                    PhysicalPortId::Ethernet(_) | PhysicalPortId::SfpSfpPlus(_) => {
                        if let Some(ethernet) =
                            self.ethernet.get_mut(&AsciiString::from(port.to_string()))
                        {
                            ethernet.name = name;
                        } else {
                            error!("Ethernet Port not defined {}", port);
                        }
                    }
                    PhysicalPortId::Wifi(_) => {}
                    PhysicalPortId::Wlan(_) => {}
                    PhysicalPortId::Loopback => {}
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
                .filter_map(|i| i.interface_name())
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
                v2_backbone.use_bfd = Some(false);
                let v3_backbone = self
                    .ospf_interface
                    .entry((b"backbone-v3".into(),))
                    .or_default();
                v3_backbone.interfaces = ports.clone();
                v3_backbone.use_bfd = Some(false);
            }
        }
    }
}
mikrotik_model!(
    name = WirelessDeviceData,
    detect = new,
    fields(wireless_cap(single = "interface/wireless/cap"),),
);

impl WirelessDeviceDataTarget {
    fn new(model: &[u8]) -> Self {
        Self {
            wireless_cap: Default::default(),
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
