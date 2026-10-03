use crate::device::ros::l2::NameGenerator;
use crate::{
    Error,
    device::ros::{
        hw_facts::build_ethernet_ports,
        l2::{L2Port, L2Setup, SimpleBridgePort},
    },
    topology::{
        IpPrefixId, PhysicalPortId,
        access::{AccessTopology, device::DeviceAccess, vxlan::VxlanAccess},
    },
};
use convert_case::{Case, Casing};
use ipnet::{IpAdd, IpNet, IpSub, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        InterfaceBridgeProtocolMode, InterfaceEthernetCfg, InterfaceEthernetPoeOut,
        InterfaceVlanByName, InterfaceVlanCfg, InterfaceVxlanByName, InterfaceVxlanCfg,
        InterfaceWifiProvisioningAction, IpAddressByAddress, IpAddressCfg, IpDhcpClientCfg,
        Ipv6AddressByAddress, Ipv6AddressCfg, RoutingOspfInstanceByName, RoutingOspfInstanceCfg,
        RoutingOspfInstanceVersion, RoutingRedistribute, VlanFrameTypes, WifiChannelBand,
        WifiManagement, YesNo,
    },
    value,
    value::HasNone,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, btree_map::Iter},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

pub mod graphql;
mod hw_facts;

pub mod l2;
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
    #[error("No AP Mgmt Interface defined")]
    MissingApMgmtIf,
    #[error("Ambigous VLAN IDs on single plane: {0:?}")]
    AmbiguousVlanIds(HashMap<u16, Vec<BTreeSet<L2Port>>>),
    #[error("Interface {0} is terminated on a VXLAN but carries no VLAN to select inside it")]
    VxlanTerminationWithoutVlan(AsciiString),
    #[error("Device has ports on VXLAN {0} but no primary IPv4 address to use as VTEP")]
    MissingVtepAddress(AsciiString),
}

impl BaseDeviceDataTarget {
    fn new(model: &[u8]) -> Result<Self, SetupError> {
        let ethernet_ports = build_ethernet_ports(model);
        if ethernet_ports.is_empty() {
            return Err(SetupError::NoPortsFound(AsciiString::from(model)));
        }
        Ok(Self {
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
        })
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
    /// Realizes an already-computed `L2Setup` as RouterOS bridge/bridge-port/
    /// bridge-vlan/interface-vlan mutations and renames each physical ethernet port
    /// to its generated name. IP/DHCP/OSPF config is handled separately via
    /// `L2Setup::logical_interfaces` (see `setup_ip_addresses`/`setup_ospf`), since
    /// none of that needs to know which of the three L2 domain kinds it came from.
    fn setup_l2<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        setup: &L2Setup<N>,
    ) -> Result<(), SetupError> {
        // VXLANs `setup_wlan_ap` already attached to `bridge-caps`; an interface can
        // only be a member of one bridge, so those stay owned by the AP path.
        let ap_managed_vnis = self
            .vxlan
            .values()
            .map(|v| v.0.vni)
            .collect::<HashSet<_>>();
        for single in setup.single_ports.iter() {
            if let Some(ethernet_port) = self.get_ethernet_port(single.port)? {
                ethernet_port.name = single.name.clone();
            }
        }
        for bridge in setup.simple_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = false;
            bridge_cfg.0.ingress_filtering = Some(false);
            bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
            for port in &bridge.ports {
                if let SimpleBridgePort::Ethernet {
                    name,
                    port: port_id,
                    ..
                } = port
                {
                    if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                        ethernet_port.name = name.clone();
                    }
                    self.bridge_port
                        .entry((bridge.name.clone(), name.clone()))
                        .or_default()
                        .frame_types = VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged;
                }
            }
        }
        for bridge in setup.tagged_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = true;
            bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
            let mut tags_of_port = HashMap::<AsciiString, (Option<u16>, Vec<u16>)>::new();
            let mut ports_of_vlan = HashMap::<u16, (Vec<AsciiString>, Vec<AsciiString>)>::new();
            for (&vlan_id, entry) in &bridge.ports {
                let is_physical_name = entry.ports.iter().any(|p| {
                    matches!(
                        p,
                        L2Port::TaggedEthernet { name, .. } | L2Port::UntaggedEthernet { name, .. }
                            if *name == entry.if_name
                    )
                });
                if !is_physical_name {
                    self.vlan.entry(entry.if_name.clone()).or_insert_with(|| {
                        InterfaceVlanByName(InterfaceVlanCfg {
                            interface: bridge.name.clone(),
                            vlan_id,
                            ..Default::default()
                        })
                    });
                    // With VLAN filtering, a VLAN interface on the bridge only sees
                    // traffic if the bridge itself is a tagged member of that VLAN.
                    ports_of_vlan
                        .entry(vlan_id)
                        .or_default()
                        .1
                        .push(bridge.name.clone());
                }
                for port in &entry.ports {
                    match port {
                        L2Port::TaggedEthernet {
                            name,
                            port: port_id,
                            ..
                        } => {
                            if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                                ethernet_port.name = name.clone();
                            }
                            tags_of_port.entry(name.clone()).or_default().1.push(vlan_id);
                            ports_of_vlan.entry(vlan_id).or_default().1.push(name.clone());
                        }
                        L2Port::UntaggedEthernet {
                            name,
                            port: port_id,
                            ..
                        } => {
                            if let Some(ethernet_port) = self.get_ethernet_port(*port_id)? {
                                ethernet_port.name = name.clone();
                            }
                            tags_of_port.entry(name.clone()).or_default().0 = Some(vlan_id);
                            ports_of_vlan.entry(vlan_id).or_default().0.push(name.clone());
                        }
                        L2Port::VxLan { name, vxlan } => {
                            let Some(vni) = vxlan.vni() else { continue };
                            if ap_managed_vnis.contains(&vni) {
                                continue;
                            }
                            let my_ip = device
                                .primary_ip_v4()
                                .and_then(|ip| ip.addr())
                                .ok_or_else(|| SetupError::MissingVtepAddress(name.clone()))?;
                            self.vxlan.entry(name.clone()).or_insert_with(|| {
                                InterfaceVxlanByName(InterfaceVxlanCfg {
                                    vni,
                                    local_address: Some(my_ip),
                                    ..Default::default()
                                })
                            });
                            for remote in vxlan.vteps().into_iter().filter(|ip| *ip != my_ip) {
                                self.vxlan_vteps
                                    .entry((name.clone(), remote))
                                    .or_default();
                            }
                            tags_of_port.entry(name.clone()).or_default().1.push(vlan_id);
                            ports_of_vlan.entry(vlan_id).or_default().1.push(name.clone());
                        }
                        L2Port::CapsMan { .. }
                        | L2Port::Cap { .. }
                        | L2Port::CapsManMgmt
                        | L2Port::CapMgmt
                        | L2Port::Ip(_) => {}
                    }
                }
            }
            for (port_name, (untagged, tagged)) in tags_of_port {
                let bridge_port = self
                    .bridge_port
                    .entry((bridge.name.clone(), port_name))
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
                        bridge.name.clone(),
                        tagged.into_iter().collect(),
                        untagged.into_iter().collect(),
                        Some(value::PossibleRangeDash::Single(vlan_id))
                            .into_iter()
                            .collect(),
                    ))
                    .or_default();
            }
        }
        Ok(())
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
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<L2Setup<N>, SetupError> {
        self.set_name_and_loopback(device);

        let l2 = L2Setup::new(device, name_generator)?;
        self.setup_ip_addresses(&l2)?;
        self.setup_ospf(device, &l2);
        self.setup_wlan_ap(device);
        self.setup_l2(device, &l2)?;
        for port in device.interfaces() {
            if let Some(port_id) = port.external_port() {
                if let Some(ethernet_port) = self.get_ethernet_port(port_id)? {
                    // `generate_ethernet()` already leaves `poe_out` at `None` for
                    // boards without PoE-out hardware (e.g. CHR, several switch
                    // models); only override it here if the baseline already
                    // established PoE capability, otherwise RouterOS rejects the
                    // whole `/interface/ethernet/set` call with "bad parameter
                    // poe-out" - breaking the rename bundled into the same call.
                    if ethernet_port.poe_out.is_some() {
                        ethernet_port.poe_out = Some(if port.enable_poe() {
                            InterfaceEthernetPoeOut::AutoOn
                        } else {
                            InterfaceEthernetPoeOut::Off
                        });
                    }
                }
            }
        }
        Ok(l2)
    }

    fn set_name_and_loopback(&mut self, device: &DeviceAccess) {
        self.set_identity(device.name());
        if let Some(loopback_ip) = device.loopback_ip().and_then(|ip| ip.addr()) {
            self.set_loopback_ip(loopback_ip);
        }
    }

    fn setup_wlan_ap(&mut self, device: &DeviceAccess) {
        if let Some(wlan_group) = device.wlan_ap_of() {
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

    fn setup_ospf<N: NameGenerator>(&mut self, device: &DeviceAccess, l2: &L2Setup<N>) {
        if let Some(router_id) = device.primary_ip_v4().and_then(|ip| ip.addr()) {
            let ports = l2
                .logical_interfaces()
                .filter(|p| p.use_ospf)
                .map(|p| p.if_name.clone())
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

    fn setup_ip_addresses<N: NameGenerator>(&mut self, l2: &L2Setup<N>) -> Result<(), SetupError> {
        for logical in l2.logical_interfaces() {
            let if_name = logical.if_name.clone();
            let ips = logical
                .addresses
                .iter()
                .filter_map(|ip| ip.net().map(|a| (a, ip.clone())))
                .collect::<Vec<_>>();
            if ips.is_empty() {
                if logical.enable_dhcp_client {
                    self.enable_dhcp_client(if_name);
                }
            } else {
                let dhcp_server = logical.enable_dhcp_server;
                for (ip_idx, (ip, ip_address)) in ips.iter().enumerate() {
                    self.set_ip_address(*ip, if_name.clone());
                    if dhcp_server {
                        let prefix = ip_address
                            .prefix()
                            .ok_or(SetupError::MissingPrefixOnIpAddress { ip: *ip })?;
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
    fn generate_from<N: NameGenerator>(&mut self, _device: &DeviceAccess, _l2: &L2Setup<N>) {}
}
mikrotik_model!(
    name = WifiDeviceData,
    detect = new,
    fields(
        cap(single = "interface/wifi/cap"),
        capsman(single = "interface/wifi/capsman"),
        datapath(by_key(path = "interface/wifi/datapath", key = name)),
        configuration(by_key(path = "interface/wifi/configuration", key = name)),
        provisioning(by_id(
            path = "interface/wifi/provisioning",
            keys(
                radio_mac,
                identity_regexp,
                common_name_regexp,
                address_ranges,
                supported_bands
            )
        )),
    ),
);
impl WifiDeviceDataTarget {
    fn new(model: &[u8]) -> Self {
        Self {
            provisioning: Default::default(),
            datapath: Default::default(),
            cap: Default::default(),
            capsman: Default::default(),
            configuration: Default::default(),
        }
    }
    fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        _l2: &L2Setup<N>,
    ) -> Result<(), SetupError> {
        if let Some(_ap) = device.wlan_ap_of() {
            todo!("generate CAPsMan/wifi config for the mgmt VLAN - not part of L2Setup completion");
            /*let mgmt_if = l2.ap_mgmt_if.as_ref().ok_or(SetupError::MissingApMgmtIf)?;
            let datapath = &mut self.datapath.entry(b"dp-caps".into()).or_default().0;
            let configuration = &mut self.configuration.entry(b"capsman".into()).or_default().0;
            let cap = &mut self.cap;
            let provisioning = self.provisioning.entry(Default::default()).or_default();

            datapath.bridge = Some(HasNone::Value(mgmt_if.bridge_if.clone()));
            datapath.disabled = false;
            configuration.datapath = Some(b"dp-caps".into());
            configuration.disabled = false;
            configuration.manager = Some(WifiManagement::Capsman);
            cap.discovery_interfaces = [mgmt_if.mgmt_if.clone()].into();
            cap.enabled = YesNo::Yes;
            cap.slaves_datapath = Some(b"dp-caps".into());
            provisioning.master_configuration = Some(b"capsman".into());
            provisioning.action = InterfaceWifiProvisioningAction::CreateDynamicEnabled;
            provisioning.disabled = false;*/
        }
        Ok(())
    }
}
fn cleanup_name(name: &str) -> String {
    name.replace(['.', '/', '+', ':'], "_")
}
