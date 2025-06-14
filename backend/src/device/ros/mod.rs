use crate::{
    Error,
    device::ros::{
        hw_facts::build_ethernet_ports,
        l2::{EndpointNameGenerator, KeepNameGenerator, L2Plane, L2Port, L2Setup, L3Access},
    },
    topology::{
        PhysicalPortId,
        access::{DeviceAccess, InterfaceAccess, VxlanAccess},
    },
};
use convert_case::{Case, Casing};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use log::error;
use mikrotik_model::{
    MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        InterfaceBridgeProtocolMode, InterfaceEthernetCfg, InterfaceEthernetPoeOut,
        InterfaceVlanByName, InterfaceVxlanByName, InterfaceVxlanCfg, IpAddressByAddress,
        IpAddressCfg, IpDhcpClientCfg, Ipv6AddressByAddress, Ipv6AddressCfg,
        RoutingOspfInstanceByName, RoutingOspfInstanceCfg, RoutingOspfInstanceVersion,
        RoutingRedistribute, VlanFrameTypes, YesNo,
    },
    value,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
};

mod graphql;
mod hw_facts;

mod l2;

mikrotik_model!(
    name = BaseDeviceData,
    //detect = new,
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
            keys(bridge, tagged, untagged, vlan_ids)
        )),
        ipv4_address(by_key(path = "ip/address", key = address)),
        ipv6_address(by_key(path = "ipv6/address", key = address)),
        vlan(by_key(path = "interface/vlan", key = name)),
        vxlan(by_key(path = "interface/vxlan", key = name)),
        vxlan_vteps(by_id(
            path = "interface/vxlan/vteps",
            keys(interface, remote_ip)
        )),
        ospf_instance(by_key(path = "routing/ospf/instance", key = name)),
        ospf_area(by_key(path = "routing/ospf/area", key = name)),
        ospf_interface(by_id(path = "routing/ospf/interface-template", keys(area))),
        dhcp_v4_client(by_id(path = "ip/dhcp-client", keys(interface))),
        ipv6_firewall_address_list(by_id(
            path = "ipv6/firewall/address-list",
            keys(address, list)
        )),
        ipv6_firewall_filter(by_id(path = "ipv6/firewall/filter", keys())),
    ),
);

const CAPS_BRIDGE_NAME: &[u8; 11] = b"bridge-caps";
const DEFAULT_BRIDGE_NAME: &[u8; 6] = b"switch";

#[derive(Error, Debug)]
pub enum SetupError {
    #[error("Problem accessing the device: {0}")]
    Access(#[from] mikrotik_model::resource::Error),
    #[error("Device is not a Routerboard device")]
    RouterboardNotDefined,
    #[error("No ports found for device type {0}")]
    NoPortsFound(AsciiString),
    #[error("Declared port {0} not found on device")]
    PortNotFound(PhysicalPortId),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SwitchVlanConcept {
    OneBridge,
}

impl BaseDeviceDataTarget {
    pub async fn detect_device(device: &MikrotikDevice) -> Result<Self, SetupError> {
        let routerboard = <mikrotik_model::model::SystemRouterboardState as mikrotik_model::resource::SingleResource>::fetch(device).await?.ok_or(SetupError::RouterboardNotDefined)?;
        Self::new(&routerboard.model.0)
    }
    fn new(model: &[u8]) -> Result<Self, SetupError> {
        let ethernet_ports = build_ethernet_ports(model);
        if ethernet_ports.is_empty() {
            return Err(SetupError::NoPortsFound(AsciiString::from(model)));
        }
        let result = Ok(Self {
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
            vlan: Default::default(),
            dhcp_v_4_client: Default::default(),
            ipv_6_firewall_address_list: Default::default(),
            ipv_6_firewall_filter: Default::default(),
        });
        result
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
    fn setup_l2(&mut self, setup: &L2Setup, concept: SwitchVlanConcept) -> Result<(), SetupError> {
        let mut plane_count_of_port = HashMap::<_, usize>::new();
        for plane in &setup.planes {
            for port in &plane.ports {
                match port {
                    L2Port::TaggedEthernet { name, .. } | L2Port::UntaggedEthernet { name, .. } => {
                        *plane_count_of_port.entry(name).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }
        let mut switch_planes = Vec::new();
        for plane in &setup.planes {
            let mut ports = Vec::new();
            let mut addresses = Vec::new();
            let mut enable_dhcp = false;
            for port in &plane.ports {
                match port {
                    L2Port::TaggedEthernet {
                        name,
                        port: port_id,
                    } => {
                        let port1 = *port_id;
                        if let Some(ethernet_port) = self.get_ethernet_port(port1)? {
                            ethernet_port.name = name.clone();
                        }
                        ();
                        ports.push(port);
                    }
                    L2Port::UntaggedEthernet {
                        name,
                        port: port_id,
                    } => {
                        let port1 = *port_id;
                        if let Some(ethernet_port) = self.get_ethernet_port(port1)? {
                            ethernet_port.name = name.clone();
                        }
                        ports.push(port);
                    }
                    L2Port::VxLan { .. } => {
                        ports.push(port);
                        todo!("Define vxlan")
                    }
                    L2Port::Caps => {
                        ports.push(port);
                        todo!("define caps")
                    }
                    L2Port::L3 {
                        access: L3Access::Ip(ip),
                        ..
                    } => {
                        addresses.push(*ip);
                    }
                    L2Port::L3 {
                        access: L3Access::DhcpClient,
                        if_name: _,
                    } => {
                        enable_dhcp = true;
                    }
                }
            }
            match ports.as_slice() {
                [L2Port::UntaggedEthernet { name, .. }]
                    if plane_count_of_port.get(name) == Some(&1) =>
                {
                    if addresses.is_empty() && enable_dhcp {
                        self.enable_dhcp_client(name.clone());
                    }
                    for addr in addresses {
                        self.set_ip_address(addr, name.clone());
                    }
                }
                _ => {
                    switch_planes.push(plane);
                }
            }
        }
        if !switch_planes.is_empty() {
            if let [single_plane] = if switch_planes.iter().all(|plane| {
                plane.ports.iter().all(|port| match port {
                    L2Port::TaggedEthernet { .. } => false,
                    L2Port::UntaggedEthernet { .. } => true,
                    L2Port::VxLan { .. } => false,
                    L2Port::Caps => false,
                    L2Port::L3 { .. } => true,
                })
            }) {
                switch_planes.as_slice()
            } else {
                &[]
            } {
                self.setup_single_switch_without_vlan(single_plane);
            } else {
                match concept {
                    SwitchVlanConcept::OneBridge => {
                        self.setup_big_bridge(switch_planes);
                    }
                }
            }
        }
        Ok(())
    }

    fn setup_big_bridge(&mut self, switch_planes: Vec<&L2Plane>) {
        let bridge = self.bridge.entry(DEFAULT_BRIDGE_NAME.into()).or_default();
        bridge.0.vlan_filtering = true;
        bridge.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
        let mut tags_of_port = HashMap::<&AsciiString, (Option<u16>, Vec<u16>)>::new();
        let mut ports_of_vlan = HashMap::<u16, (Vec<&AsciiString>, Vec<&AsciiString>)>::new();
        for plane in switch_planes {
            for port in &plane.ports {
                match port {
                    L2Port::TaggedEthernet { name, .. } => {
                        tags_of_port.entry(name).or_default().1.push(plane.vlan_id);
                        ports_of_vlan.entry(plane.vlan_id).or_default().1.push(name);
                    }
                    L2Port::UntaggedEthernet { name, .. } => {
                        tags_of_port.entry(name).or_default().0 = Some(plane.vlan_id);
                        ports_of_vlan.entry(plane.vlan_id).or_default().0.push(name);
                    }
                    L2Port::VxLan { .. } => {}
                    L2Port::Caps => {}
                    L2Port::L3 { access, if_name } => {
                        let if_name = if let Some(if_name) = if_name {
                            if_name.clone()
                        } else {
                            format!("switch-vlan-{}", plane.vlan_id).as_str().into()
                        };
                        match access {
                            L3Access::Ip(ip) => {
                                self.set_ip_address(*ip, if_name.clone());
                            }
                            L3Access::DhcpClient => {
                                self.enable_dhcp_client(if_name.clone());
                            }
                        }

                        let vlan_cfg = &mut self
                            .vlan
                            .entry(if_name)
                            .or_insert(InterfaceVlanByName(Default::default()))
                            .0;
                        vlan_cfg.interface = DEFAULT_BRIDGE_NAME.into();
                        vlan_cfg.vlan_id = plane.vlan_id;
                    }
                }
            }
        }
        for (port, (untagged, tagged)) in tags_of_port {
            let bridge_port = self
                .bridge_port
                .entry((AsciiString::from(DEFAULT_BRIDGE_NAME), port.clone()))
                .or_default();
            bridge_port.ingress_filtering = true;
            bridge_port.frame_types = if let Some(untagged_id) = untagged {
                bridge_port.pvid = untagged_id;
                if tagged.is_empty() {
                    VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged
                } else {
                    VlanFrameTypes::AdmitAll
                }
            } else {
                VlanFrameTypes::AdmitOnlyVlanTagged
            };
        }
        for (vlan_id, (untagged, tagged)) in ports_of_vlan {
            self.bridge_vlan
                .entry((
                    AsciiString::from(DEFAULT_BRIDGE_NAME),
                    tagged.into_iter().cloned().collect(),
                    untagged.into_iter().cloned().collect(),
                    Some(value::PossibleRangeDash::Single(vlan_id))
                        .into_iter()
                        .collect(),
                ))
                .or_default();
        }
    }

    fn setup_single_switch_without_vlan(&mut self, single_plane: &L2Plane) {
        let bridge = self.bridge.entry(DEFAULT_BRIDGE_NAME.into()).or_default();
        bridge.0.vlan_filtering = false;
        bridge.0.ingress_filtering = Some(false);
        bridge.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
        for port in &single_plane.ports {
            match port {
                L2Port::TaggedEthernet { .. } => {
                    panic!("Cannot create tagged port on switch without vlan")
                }
                L2Port::UntaggedEthernet { name, .. } => {
                    self.bridge_port
                        .entry((DEFAULT_BRIDGE_NAME.into(), name.clone()))
                        .or_default()
                        .frame_types = VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged;
                }
                L2Port::VxLan { .. } => {
                    panic!("Cannot create tagged port on switch without vlan")
                }
                L2Port::Caps => {
                    panic!("Cannot create tagged port on switch without vlan")
                }
                L2Port::L3 {
                    access: L3Access::Ip(ip),
                    ..
                } => {
                    self.set_ip_address(*ip, DEFAULT_BRIDGE_NAME);
                }
                L2Port::L3 {
                    access: L3Access::DhcpClient,
                    ..
                } => {
                    self.enable_dhcp_client(DEFAULT_BRIDGE_NAME.into());
                }
            }
        }
    }

    fn get_ethernet_port(
        &mut self,
        port: PhysicalPortId,
    ) -> Result<Option<&mut InterfaceEthernetCfg>, SetupError> {
        Ok(if let Some(default_name) = port.default_name() {
            Some(
                self.ethernet
                    .get_mut(&default_name)
                    .ok_or(SetupError::PortNotFound(port))?,
            )
        } else {
            None
        })
    }

    fn set_ip_address(&mut self, ip: IpNet, if_name: impl Into<AsciiString>) {
        match ip {
            IpNet::V4(v4_net) => {
                self.ipv_4_address
                    .entry(v4_net)
                    .or_insert(IpAddressByAddress(IpAddressCfg {
                        address: Default::default(),
                        interface: Default::default(),
                        comment: None,
                    }))
                    .0
                    .interface = if_name.into();
            }
            IpNet::V6(v6_net) => {
                self.ipv_6_address
                    .entry(v6_net)
                    .or_insert(Ipv6AddressByAddress(Ipv6AddressCfg {
                        address: Default::default(),
                        advertise: false,
                        auto_link_local: false,
                        comment: None,
                        disabled: false,
                        eui_64: false,
                        from_pool: None,
                        interface: Default::default(),
                        no_dad: false,
                    }))
                    .0
                    .interface = if_name.into();
            }
        }
    }
    fn generate_from(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        self.set_identity(device.name());
        if let Some(loopback_ip) = device.loopback_ip() {
            self.set_loopback_ip(loopback_ip);
        }

        let l2 = L2Setup::new(device, &mut EndpointNameGenerator);
        self.setup_l2(&l2, SwitchVlanConcept::OneBridge)?;
        for port in device.interfaces() {
            if let Some(port_id) = port.external_port() {
                if let Some(ethernet_port) = self.get_ethernet_port(port_id)? {
                    ethernet_port.poe_out = if port.enable_poe() {
                        Some(InterfaceEthernetPoeOut::AutoOn)
                    } else {
                        Some(InterfaceEthernetPoeOut::Off)
                    };
                }
            }
        }
        self.setup_ospf(device);
        self.setup_wlan_ap(device);
        Ok(())
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
                    BTreeSet::default(),
                    BTreeSet::from([value::PossibleRangeDash::Range {
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
                v2_instance.0.router_id = Some(router_id.to_string().into());
                v2_instance.0.version = RoutingOspfInstanceVersion::_2;
                let v3_instance = self
                    .ospf_instance
                    .entry(b"default-v3".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v3_instance.0.redistribute =
                    [RoutingRedistribute::Connected, RoutingRedistribute::Static]
                        .into_iter()
                        .collect();
                v3_instance.0.router_id = Some(router_id.to_string().into());
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

    fn enable_dhcp_client(&mut self, if_name: AsciiString) {
        self.dhcp_v_4_client
            .entry((if_name,))
            .or_insert(IpDhcpClientCfg {
                interface: Default::default(),
                add_default_route: YesNo::Yes,
                comment: None,
                default_route_distance: Some(10),
                dhcp_options: Default::default(),
                script: None,
                use_peer_dns: true,
                use_peer_ntp: false,
            });
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
