use crate::{
    Error,
    device::ros::{
        hw_facts::build_ethernet_ports,
        l2::{EndpointNameGenerator, L2Plane, L2Port, L2Setup},
    },
    topology::{
        IpPrefixId, PhysicalPortId,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess, vxlan::VxlanAccess,
        },
    },
};
use convert_case::{Case, Casing};
use ipnet::{IpAdd, IpNet, IpSub, Ipv4Net, Ipv6Net};
use log::error;
use mikrotik_model::{
    MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        InterfaceBridgeProtocolMode, InterfaceEthernetCfg, InterfaceEthernetPoeOut,
        InterfaceVlanByName, InterfaceVlanCfg, InterfaceVxlanByName, InterfaceVxlanCfg,
        IpAddressByAddress, IpAddressCfg, IpDhcpClientCfg, Ipv6AddressByAddress, Ipv6AddressCfg,
        RoutingOspfInstanceByName, RoutingOspfInstanceCfg, RoutingOspfInstanceVersion,
        RoutingRedistribute, VlanFrameTypes, YesNo,
    },
    value,
};
use std::{
    collections::{
        BTreeMap, BTreeSet, HashMap, HashSet,
        btree_map::{Entry, Iter},
    },
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

mod graphql;
mod hw_facts;

mod l2;
#[cfg(test)]
mod test;

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
        dhcp_v4_server(by_key(path = "ip/dhcp-server", key = name)),
        dhcp_v4_server_network(by_key(path = "ip/dhcp-server/network", key = address)),
        ipv4_pool(by_key(path = "ip/pool", key = name)),
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
    #[error("Missing Prefix configuration for ip address: {ip}")]
    MissingPrefixOnIpAddress { ip: IpNet },
    #[error("Address on Prefix {prefix} not found")]
    MissingAddressOnPrefix { prefix: IpPrefixId },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SwitchVlanConcept {
    OneBridge,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
enum MappedPlane {
    Tagged { if_name: AsciiString, vid: u16 },
    Untagged(AsciiString),
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
            dhcp_v_4_server: Default::default(),
            dhcp_v_4_server_network: Default::default(),
            ipv_4_pool: Default::default(),
        });
        result
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
    fn setup_l2(
        &mut self,
        setup: &L2Setup,
        concept: SwitchVlanConcept,
        mapped_planes: &mut Vec<(InterfaceAccess, MappedPlane)>,
    ) -> Result<(), SetupError> {
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
                }
            }
            match ports.as_slice() {
                [L2Port::UntaggedEthernet { name, .. }]
                    if plane_count_of_port.get(name) == Some(&1) =>
                {
                    mapped_planes
                        .push((plane.root_port.clone(), MappedPlane::Untagged(name.clone())));
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
                })
            }) {
                switch_planes.as_slice()
            } else {
                &[]
            } {
                self.setup_single_switch_without_vlan(single_plane, mapped_planes);
            } else {
                match concept {
                    SwitchVlanConcept::OneBridge => {
                        self.setup_big_bridge(switch_planes, mapped_planes);
                    }
                }
            }
        }
        Ok(())
    }

    fn setup_big_bridge(
        &mut self,
        switch_planes: Vec<&L2Plane>,
        mapped_planes: &mut Vec<(InterfaceAccess, MappedPlane)>,
    ) {
        let bridge = self.bridge.entry(DEFAULT_BRIDGE_NAME.into()).or_default();
        bridge.0.vlan_filtering = true;
        bridge.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
        let mut tags_of_port = HashMap::<&AsciiString, (Option<u16>, Vec<u16>)>::new();
        let mut ports_of_vlan = HashMap::<u16, (Vec<&AsciiString>, Vec<&AsciiString>)>::new();
        for plane in switch_planes {
            mapped_planes.push((
                plane.root_port.clone(),
                MappedPlane::Tagged {
                    if_name: DEFAULT_BRIDGE_NAME.into(),
                    vid: plane.vlan_id,
                },
            ));
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

    fn setup_single_switch_without_vlan(
        &mut self,
        single_plane: &L2Plane,
        mapped_planes: &mut Vec<(InterfaceAccess, MappedPlane)>,
    ) {
        let bridge = self.bridge.entry(DEFAULT_BRIDGE_NAME.into()).or_default();
        bridge.0.vlan_filtering = false;
        bridge.0.ingress_filtering = Some(false);
        bridge.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
        mapped_planes.push((
            single_plane.root_port.clone(),
            MappedPlane::Untagged(DEFAULT_BRIDGE_NAME.into()),
        ));
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
        if let Some(loopback_ip) = device.loopback_ip().and_then(|ip| ip.addr()) {
            self.set_loopback_ip(loopback_ip);
        }

        let l2 = L2Setup::new(device, &mut EndpointNameGenerator);
        let mut mapped_planes = Vec::new();
        self.setup_l2(&l2, SwitchVlanConcept::OneBridge, &mut mapped_planes)?;
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
        self.setup_ip_addresses(&mapped_planes);
        self.setup_ospf(device, &mapped_planes);
        self.setup_wlan_ap(device);
        Ok(())
    }

    fn setup_wlan_ap(&mut self, device: &DeviceAccess) {
        if let Some(wlan_group) = device.wlan_ap_of() {
            let bridge_caps = self.bridge.entry(CAPS_BRIDGE_NAME.into()).or_default();
            bridge_caps.0.vlan_filtering = true;
            bridge_caps.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
            if let Some(my_ip) = device.primary_ip_v4().and_then(|ip| ip.addr()) {
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

    fn setup_vxlan(&mut self, vxlan: VxlanAccess, my_ip: &IpAddr) {
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

    fn if_of_mapped_plane(&mut self, plane: &MappedPlane) -> AsciiString {
        match plane {
            MappedPlane::Tagged { if_name, vid } => {
                let mut vlan_port_name = Vec::from(if_name.0.as_ref());
                vlan_port_name.push(b'-');
                vlan_port_name.extend(AsciiString::from(vid.to_string()).0);
                let vlan_port_name = AsciiString::from(vlan_port_name.as_slice());
                if let Entry::Vacant(vlan_port_entry) = self.vlan.entry(vlan_port_name.clone()) {
                    vlan_port_entry.insert(InterfaceVlanByName(InterfaceVlanCfg {
                        interface: if_name.clone(),
                        vlan_id: *vid,
                        ..Default::default()
                    }));
                }
                vlan_port_name
            }
            MappedPlane::Untagged(if_name) => if_name.clone(),
        }
    }

    fn setup_ospf(&mut self, device: &DeviceAccess, planes: &[(InterfaceAccess, MappedPlane)]) {
        if let Some(router_id) = device.primary_ip_v4().and_then(|ip| ip.addr()) {
            let ports = planes
                .iter()
                .filter(|(p, _)| p.use_ospf())
                .map(|(_, map)| self.if_of_mapped_plane(map))
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
                v3_backbone.interfaces = ports;
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

    fn setup_ip_addresses(
        &mut self,
        mapped_planes: &[(InterfaceAccess, MappedPlane)],
    ) -> Result<(), SetupError> {
        for (if_access, plane) in mapped_planes {
            let ips = if_access.ips();
            if ips.is_empty() {
                if if_access.is_enable_dhcp_client() {
                    let if_name = self.if_of_mapped_plane(plane);
                    self.enable_dhcp_client(if_name);
                }
            } else {
                let if_name = self.if_of_mapped_plane(plane);
                let dhcp_server = if_access.is_enable_dhcp_server();
                for (ip_idx, ip_address) in ips.iter().enumerate() {
                    if let Some(ip) = ip_address.net() {
                        self.set_ip_address(ip, if_name.clone());
                        if dhcp_server {
                            let prefix = ip_address
                                .prefix()
                                .ok_or(SetupError::MissingPrefixOnIpAddress { ip })?;
                            if let (IpNet::V4(ip), IpNet::V4(net)) = (
                                ip,
                                prefix.prefix().ok_or(SetupError::MissingAddressOnPrefix {
                                    prefix: prefix.id(),
                                })?,
                            ) {
                                let mut dhcp_ranges_explicit = Vec::new();
                                let mut gap_finder = GapFinder::<Ipv4Addr>::new();
                                for range in prefix.ranges() {
                                    if let (Some(IpAddr::V4(start)), Some(IpAddr::V4(end))) =
                                        (range.start(), range.end())
                                    {
                                        if range.is_dhcp() {
                                            dhcp_ranges_explicit.push(start..end);
                                        }
                                        gap_finder.reserve_ipv4_range(start..end);
                                    }
                                }
                                let dhcp_ranges = if dhcp_ranges_explicit.is_empty() {
                                    for child_prefix in prefix.children() {
                                        if let Some(IpNet::V4(net)) = child_prefix.prefix() {
                                            gap_finder.reserve_ipv4_net(net);
                                        }
                                    }
                                    for ip in prefix.ips() {
                                        if let Some(IpAddr::V4(ip)) = ip.addr() {
                                            gap_finder.reserve_ipv4(ip)
                                        }
                                    }

                                    gap_finder.find_gaps_ipv4(net).collect()
                                } else {
                                    dhcp_ranges_explicit
                                };
                                if !dhcp_ranges.is_empty() {
                                    let suffix = if ip_idx == 0 {
                                        if_name.to_string()
                                    } else {
                                        format!("{if_name}-{ip_idx}")
                                    };
                                    let server_name: AsciiString = format!("dhcp-{suffix}").into();
                                    let server = &mut self
                                        .dhcp_v_4_server
                                        .entry(server_name.clone())
                                        .or_default()
                                        .0;
                                    server.interface = if_name.clone();
                                    server.address_pool = server_name.clone();
                                    let network =
                                        &mut self.dhcp_v_4_server_network.entry(net).or_default().0;
                                    network.gateway.insert(ip.addr());
                                    network.dns_server.insert(ip.addr());
                                    self.ipv_4_pool.entry(server_name).or_default().0.ranges =
                                        dhcp_ranges
                                            .iter()
                                            .map(|r| format!("{}-{}", r.start, r.end).into())
                                            .collect();
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct GapFinder<V: Ord + Copy> {
    reserved_chunks: BTreeMap<V, i32>,
}

impl<V: Ord + Copy> GapFinder<V> {
    pub fn new() -> Self {
        GapFinder {
            reserved_chunks: BTreeMap::new(),
        }
    }
    pub fn reserve(&mut self, chunk: Range<V>) {
        *self.reserved_chunks.entry(chunk.start).or_insert(0) += 1;
        *self.reserved_chunks.entry(chunk.end).or_insert(0) -= 1;
    }
    pub fn gaps(&self, Range { start, end }: Range<V>) -> GapIterator<V> {
        GapIterator {
            current_value: 0,
            last_key: start,
            start,
            end,
            gap_iter: self.reserved_chunks.iter(),
        }
    }
}

impl GapFinder<Ipv4Addr> {
    pub fn reserve_ipv4(&mut self, ip: Ipv4Addr) {
        self.reserve(ip.saturating_sub(1)..ip.saturating_add(1))
    }
    pub fn reserve_ipv4_range(&mut self, ip: Range<Ipv4Addr>) {
        self.reserve(ip.start.saturating_sub(1)..ip.end.saturating_add(1))
    }
    pub fn reserve_ipv4_net(&mut self, ip: Ipv4Net) {
        self.reserve_ipv4_range(ip.network()..ip.broadcast());
    }
    pub fn find_gaps_ipv4(&self, net: Ipv4Net) -> GapIterator<Ipv4Addr> {
        self.gaps(net.network().saturating_add(1)..net.broadcast().saturating_sub(1))
    }
}
impl GapFinder<Ipv6Addr> {
    pub fn reserve_ipv6(&mut self, ip: Ipv6Addr) {
        self.reserve(ip.saturating_sub(1)..ip.saturating_add(1))
    }
}
impl GapFinder<IpAddr> {
    pub fn reserve_ip(&mut self, ip: IpAddr) {
        self.reserve(match ip {
            IpAddr::V4(ip) => IpAddr::V4(ip.saturating_sub(1))..IpAddr::V4(ip.saturating_add(1)),
            IpAddr::V6(ip) => IpAddr::V6(ip.saturating_sub(1))..IpAddr::V6(ip.saturating_add(1)),
        });
    }
}
struct GapIterator<'a, V: Ord + Copy> {
    current_value: i32,
    last_key: V,
    start: V,
    end: V,
    gap_iter: Iter<'a, V, i32>,
}
impl<'a, V: Ord + Copy> Iterator for GapIterator<'a, V> {
    type Item = Range<V>;

    fn next(&mut self) -> Option<Self::Item> {
        for (current_key, increment) in self.gap_iter.by_ref() {
            let current_key = (*current_key).clamp(self.start, self.end);
            let found_gap = if self.current_value <= 0 && current_key > self.last_key {
                Some(self.last_key..current_key)
            } else {
                None
            };
            self.current_value += increment;
            self.last_key = current_key;
            if let Some(found_gap) = found_gap {
                return Some(found_gap);
            }
        }
        if self.current_value <= 0 && self.end > self.last_key {
            let last_key = self.last_key;
            self.last_key = self.end;
            Some(last_key..self.end)
        } else {
            None
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
