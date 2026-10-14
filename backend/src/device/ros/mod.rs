use crate::device::ros::l2::NameGenerator;
use crate::device::ros::warning::{ApplyWarning, PoeCut, RadioBound};
use crate::{
    Error,
    device::ros::{
        hw_facts::{
            RadioPackage, build_ethernet_ports, build_w60g_ports, build_wireless_ports,
            radio_package, runs_wireless,
        },
        l2::{L2Port, L2Setup, SimpleBridgePort},
    },
    topology::{
        IpPrefixId, PhysicalPortId, Radio, RadioLink, RadioRole, WlanAuth,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
            wlan_group::WlanGroupAccess,
        },
    },
};
use ipnet::{IpAdd, IpNet, IpSub, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        EnabledDisabled, InterfaceBridgeProtocolMode, InterfaceEthernetByDefaultName,
        InterfaceEthernetCfg, InterfaceEthernetPoeOut, InterfaceVlanByName, InterfaceVlanCfg,
        InterfaceVxlanByName, InterfaceVxlanCfg, InterfaceW60GMode, InterfaceW60GRegion,
        CapsAuthenticationTypes, CapsEncryption, CapsManConfigurationMode,
        CapsManDatapathVlanMode, CapsManProvisioningAction, CapsManProvisioningCfg,
        CapsManProvisioningHwSupportedModes,
        WifiCountryLower as CapsManWifiCountry, InterfaceWifiCapsmanUpgradePolicy, InterfaceWifiProvisioningAction, InterfaceWirelessCfg, InterfaceWirelessChannelWidth,
        InterfaceWirelessFrequencyMode, InterfaceWirelessMode, InterfaceWirelessTxPowerMode,
        InterfaceWirelessWirelessProtocol::{Nv2, Nv2Nstreme80211},
        IpAddressByAddress, IpAddressCfg, IpDhcpClientCfg, IpDhcpClientDhcpOptions,
        IpFirewallFilterAction, IpFirewallFilterCfg, IpFirewallNatAction, IpFirewallNatCfg,
        Ipv6NdAdvertiseDns, Ipv6NdByInterface, Ipv6NdCfg,
        IpSocksAuthMethod, IpSocksUsersByName, IpSshCfg, IpSshForwardingEnabled, IpSocksUsersCfg, Ipv6AddressByAddress,
        Ipv6AddressCfg, RoutingOspfInstanceByName, RoutingOspfInstanceCfg,
        RoutingOriginateDefault, RoutingOspfInstanceVersion, RoutingRedistribute, VlanFrameTypes, WifiChannelBand,
        WifiAuthenticationTypes, WifiCountry, WifiManagement, YesNo,
    },
    value,
    value::{Auto, HasNone, ParseRosValueResult, RosValue},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, btree_map::Iter},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

pub mod fallback;
pub mod graphql;
pub(crate) mod hw_facts;
pub mod warning;

#[cfg(test)]
mod capsman_test;
pub mod l2;
#[cfg(test)]
mod mtu_test;
#[cfg(test)]
mod netbox_example_test;
#[cfg(test)]
mod poe_test;
#[cfg(test)]
mod radio_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod wlan_vlans_test;

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
        ip_firewall_filter(by_id(
            path = "ip/firewall/filter",
            keys(chain, in_interface, protocol, dst_port)
        )),
        ip_firewall_nat(by_id(path = "ip/firewall/nat", keys(chain, out_interface))),
        dns(single = "ip/dns"),
        ipv6_nd(by_key(path = "ipv6/nd", key = interface)),
        dhcp_v4_server(by_key(path = "ip/dhcp-server", key = name)),
        dhcp_v4_server_network(by_key(path = "ip/dhcp-server/network", key = address)),
        ipv4_pool(by_key(path = "ip/pool", key = name)),
        socks(single = "ip/socks"),
        socks_users(by_key(path = "ip/socks/users", key = name)),
        ssh(single = "ip/ssh"),
        ipv6_settings(single = "ipv6/settings"),
    ),
);

pub(crate) const CAPS_BRIDGE_NAME: &[u8; 11] = b"bridge-caps";

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
    #[error(
        "VLAN {vlan} is tunneled over VXLAN and also cabled from {interface} to {far_end}, which would loop"
    )]
    VlanOnCableAndTunnel {
        vlan: Box<str>,
        interface: Box<str>,
        far_end: String,
    },
    #[error("VLAN {vlan} would join several IP networks in one L2 domain: {}", networks.join("; "))]
    MultipleNetworksOnVlan {
        vlan: Box<str>,
        networks: Vec<String>,
    },
    #[error("VLAN {vlan} would have several DHCP servers in one L2 domain: {}", servers.join(", "))]
    MultipleDhcpServersOnVlan {
        vlan: Box<str>,
        servers: Vec<String>,
    },
    #[error("Device has ports on VXLAN {0} but no primary IPv4 address to use as VTEP")]
    MissingVtepAddress(AsciiString),
    #[error(
        "Port {0} would be a member of more than one bridge (its VLANs are split across bridges)"
    )]
    PortInMultipleBridges(AsciiString),
    #[error("Unsupported RF channel width {0} MHz for a wireless radio")]
    UnsupportedChannelWidth(u32),
    #[error("Radio {0} has an RF role, but the board has no point-to-point radio profile")]
    NoRadioProfile(PhysicalPortId),
    #[error("Radio {0} has an RF role, but is not an end of a wireless link")]
    MissingWirelessLink(PhysicalPortId),
    #[error("Wireless link {0} is encrypted, but NetBox returned no pre-shared key")]
    MissingPsk(Box<str>),
    #[error(
        "Device {0} has a radio, but its site has no region (country) and its link no rf_country"
    )]
    MissingCountry(Box<str>),
    #[error("No RouterOS country known for region {0}, set rf_country on the wireless link")]
    UnknownCountry(Box<str>),
    #[error("Unknown rf_frequency_mode {0}")]
    InvalidFrequencyMode(Box<str>),
    #[error("RouterOS package {0} is not installed (update RouterOS to install it)")]
    MissingPackage(&'static str),
    #[error("{0} is an AP of a Wireless LAN Group whose controller has no loopback address")]
    MissingController(Box<str>),
    #[error(
        "the WLANs of {0}'s Wireless LAN Group have different VLANs, which its 802.11ac radios \
         (wifi-qcom-ac) can't tag: give it the radios of the legacy CAPsMAN in NetBox (wlan1, \
         wlan2 instead of wifi1, wifi2)"
    )]
    WlanVlansNotTagged(Box<str>),
    #[error(
        "{port} has MTU {mtu}, but the other end of its cable, {far_end}, {far_mtu}: OSPF needs both equal"
    )]
    MtuMismatch {
        port: Box<str>,
        mtu: u16,
        far_end: String,
        far_mtu: u16,
    },
    #[error("{0} has an MTU in NetBox, which is only set on Ethernet ports and wlan radios")]
    MtuNotSupported(Box<str>),
}

/// RouterOS's L3 MTU of a port NetBox gives none.
const DEFAULT_MTU: u16 = 1500;
/// What the L2 MTU leaves on top of the L3 MTU, for a VLAN tag.
const VLAN_TAG_SIZE: u16 = 4;

/// The L3 MTU NetBox gives `port`, the same as the far ends of its cable have: OSPF only
/// forms an adjacency between equal MTUs, and RouterOS 7 has no `mtu-ignore`.
fn port_mtu(port: &InterfaceAccess) -> Result<Option<u16>, SetupError> {
    let mtu = port.mtu();
    for far_end in port.connected_interfaces() {
        let far_mtu = far_end.mtu().unwrap_or(DEFAULT_MTU);
        if far_mtu != mtu.unwrap_or(DEFAULT_MTU) {
            return Err(SetupError::MtuMismatch {
                port: port.name().into(),
                mtu: mtu.unwrap_or(DEFAULT_MTU),
                far_end: format!(
                    "{} {}",
                    far_end.device().map(|d| d.name().to_string()).unwrap_or_default(),
                    far_end.name()
                ),
                far_mtu,
            });
        }
    }
    Ok(mtu)
}

/// The L2 MTU of a port with the L3 MTU `mtu`: the board's (`l2mtu`), raised to carry `mtu`
/// with a VLAN tag.
fn l2mtu_for(l2mtu: Option<u16>, mtu: Option<u16>) -> Option<u16> {
    match mtu {
        Some(mtu) => Some(l2mtu.unwrap_or_default().max(mtu + VLAN_TAG_SIZE)),
        None => l2mtu,
    }
}

impl BaseDeviceDataTarget {
    fn new(model: &[u8]) -> Result<Self, SetupError> {
        let ethernet_ports = build_ethernet_ports(model);
        if ethernet_ports.is_empty() {
            return Err(SetupError::NoPortsFound(AsciiString::from(model)));
        }
        Ok(Self::with_ethernet_ports(ethernet_ports))
    }
    /// An empty target for a device with exactly these ethernet ports, independent
    /// of any known board model.
    fn with_ethernet_ports(
        ethernet_ports: impl IntoIterator<Item = InterfaceEthernetByDefaultName>,
    ) -> Self {
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
            vlan: Default::default(),
            dhcp_v_4_client: Default::default(),
            ipv_6_firewall_address_list: Default::default(),
            ipv_6_firewall_filter: Default::default(),
            ip_firewall_filter: Default::default(),
            ip_firewall_nat: Default::default(),
            dns: Default::default(),
            ipv_6_nd: Default::default(),
            dhcp_v_4_server: Default::default(),
            dhcp_v_4_server_network: Default::default(),
            ipv_4_pool: Default::default(),
            socks: Default::default(),
            socks_users: Default::default(),
            // any device may be the neighbor a new one is set up from: the provisioner reaches
            // the new device through an SSH tunnel to it (`device::bootstrap`)
            ssh: IpSshCfg {
                forwarding_enabled: IpSshForwardingEnabled::Local,
                ..Default::default()
            },
            // every device gets OSPFv3 and its IPv6 addresses from NetBox; with IPv6 off they
            // are invalid and OSPFv3 never comes up, while the diff of them looks applied
            ipv_6_settings: Default::default(),
        }
    }
    /// Makes this device the provisioner's way into the network: `/ip socks` as SOCKS5, only
    /// for this user, since the proxy is reachable from wherever the device is.
    pub fn setup_management_proxy(&mut self, user: &str, password: &str) {
        self.socks.enabled = true;
        self.socks.version = 5;
        self.socks.auth_method = IpSocksAuthMethod::Password;
        self.socks_users.insert(
            user.into(),
            IpSocksUsersByName(IpSocksUsersCfg {
                name: user.into(),
                password: password.into(),
                only_one: false,
                disabled: false,
            }),
        );
    }
    fn set_identity(&mut self, name: impl Into<AsciiString>) {
        self.identity.name = name.into();
    }
    /// Realizes an already-computed `L2Setup` as RouterOS bridge/bridge-port/
    /// bridge-vlan/interface-vlan mutations and renames each physical ethernet port
    /// to its generated name. IP/DHCP/OSPF config is handled separately via
    /// `L2Setup::logical_interfaces` (see `setup_ip_addresses`/`setup_ospf`), since
    /// none of that needs to know which of the three L2 domain kinds it came from.
    /// `radio_package` is the one of the device's radios, if NetBox uses them: on a CAP whose
    /// radios don't tag VLANs, a radio without a bridge of its own is an access port of the
    /// WLANs' VLAN (`WifiDeviceDataTarget::generate_from`).
    fn setup_l2<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        setup: &L2Setup<N>,
        radio_package: Option<RadioPackage>,
    ) -> Result<(), SetupError> {
        let radio_access_ports = if device.wifi_cap_of().is_some()
            && radio_package.is_some_and(|package| package.is_wifi() && !package.tags_vlans())
        {
            device
                .interfaces()
                .iter()
                .filter_map(|interface| match interface.external_port() {
                    Some(port @ PhysicalPortId::Wifi(_)) => match setup.placement_of_port(port) {
                        Some((name, None)) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for single in setup.single_ports.iter() {
            if let Some(ethernet_port) = self.get_ethernet_port(single.port)? {
                ethernet_port.name = single.name.clone();
            }
        }
        for bridge in setup.simple_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = false;
            bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Rstp;
            // Without vlan filtering RouterOS accepts the vlan fields but never reports them
            // (nor the mstp ones without mstp), so a value here would be a diff forever; mvrp
            // is reported by some versions (7.20), not by others (7.24), and keeps whatever it is.
            bridge_cfg.0.ether_type = None;
            bridge_cfg.0.frame_types = None;
            bridge_cfg.0.ingress_filtering = None;
            bridge_cfg.0.pvid = None;
            bridge_cfg.0.mvrp = None;
            bridge_cfg.0.max_hops = None;
            bridge_cfg.0.region_revision = None;
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
                    // the radio of a CAP joins the bridge through its datapath
                    // (`WifiDeviceDataTarget::generate_from`)
                    if matches!(port_id, PhysicalPortId::Wifi(_)) && device.wifi_cap_of().is_some()
                    {
                        continue;
                    }
                    let bridge_port = self
                        .bridge_port
                        .entry((bridge.name.clone(), name.clone()))
                        .or_default();
                    bridge_port.frame_types = VlanFrameTypes::AdmitOnlyUntaggedAndPriorityTagged;
                    // a `/interface/wireless` port has no hardware offloading, and RouterOS
                    // doesn't report `hw` for it (seen on 7.20.8)
                    if matches!(port_id, PhysicalPortId::Wlan(_)) {
                        bridge_port.hw = None;
                    }
                }
            }
        }
        for bridge in setup.tagged_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = true;
            bridge_cfg.0.mvrp = Some(false);
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
                if !is_physical_name && entry.has_l3() {
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
                            tags_of_port
                                .entry(name.clone())
                                .or_default()
                                .1
                                .push(vlan_id);
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .1
                                .push(name.clone());
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
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .0
                                .push(name.clone());
                        }
                        L2Port::VxLan { name, vxlan } => {
                            let Some(vni) = vxlan.vni() else { continue };
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
                                self.vxlan_vteps.entry((name.clone(), remote)).or_default();
                            }
                            tags_of_port
                                .entry(name.clone())
                                .or_default()
                                .1
                                .push(vlan_id);
                            ports_of_vlan
                                .entry(vlan_id)
                                .or_default()
                                .1
                                .push(name.clone());
                        }
                        L2Port::Cap { .. } => {
                            for name in &radio_access_ports {
                                let untagged = &mut ports_of_vlan.entry(vlan_id).or_default().0;
                                if !untagged.contains(name) {
                                    untagged.push(name.clone());
                                }
                                tags_of_port.entry(name.clone()).or_default().0 = Some(vlan_id);
                            }
                        }
                        L2Port::CapsMan { .. }
                        | L2Port::CapsManMgmt
                        | L2Port::CapMgmt
                        | L2Port::Ip { .. } => {}
                    }
                }
            }
            for (port_name, (untagged, tagged)) in tags_of_port {
                let is_radio = radio_access_ports.contains(&port_name);
                let bridge_port = self
                    .bridge_port
                    .entry((bridge.name.clone(), port_name))
                    .or_default();
                bridge_port.ingress_filtering = true;
                // a radio has no hardware offloading, RouterOS doesn't report `hw` for it
                if is_radio {
                    bridge_port.hw = None;
                }
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
                        // RouterOS only accepts `auto-link-local=no` on a link-local
                        // address (it then replaces the generated one) and rejects it
                        // on any other with "selected address is not link local".
                        auto_link_local: !v6_net.addr().is_unicast_link_local(),
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
        self.setup_dns(device);
        self.setup_ospf(device, &l2);
        self.setup_l2(device, &l2, None)?;
        self.setup_mtu(device)?;
        self.setup_poe(device)?;
        Ok(l2)
    }

    /// PoE out of every port NetBox has: `auto-on` with `poe_mode=pse`, else `off`.
    fn setup_poe(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
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

    /// The L3 MTU NetBox gives an Ethernet port, with its L2 MTU raised to carry it (a
    /// `wlan` radio gets it in `WirelessDeviceDataTarget`).
    fn setup_mtu(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        for port in device.interfaces() {
            match (port.external_port(), port_mtu(&port)?) {
                (_, None) | (Some(PhysicalPortId::Wlan(_)), _) => {}
                (Some(port_id), Some(mtu)) => {
                    let ethernet_port = self
                        .get_ethernet_port(port_id)?
                        .ok_or_else(|| SetupError::MtuNotSupported(port.name().into()))?;
                    ethernet_port.mtu = mtu;
                    ethernet_port.l_2_mtu = l2mtu_for(ethernet_port.l_2_mtu, Some(mtu));
                }
                (None, Some(_)) => return Err(SetupError::MtuNotSupported(port.name().into())),
            }
        }
        Ok(())
    }

    fn set_name_and_loopback(&mut self, device: &DeviceAccess) {
        self.set_identity(device.name());
        if let Some(loopback_ip) = device.loopback_ip().and_then(|ip| ip.addr()) {
            self.set_loopback_ip(loopback_ip);
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
                // only where NetBox says so: a default route a device has from an uplink to
                // another network would otherwise go out into that network too
                let originate_default = Some(if device.propagates_default_route() {
                    RoutingOriginateDefault::IfInstalled
                } else {
                    RoutingOriginateDefault::Never
                });
                let v2_instance = self
                    .ospf_instance
                    .entry(b"default-v2".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v2_instance.0.redistribute =
                    [RoutingRedistribute::Connected, RoutingRedistribute::Static]
                        .into_iter()
                        .collect();
                v2_instance.0.router_id = Some(router_id.to_string().into());
                v2_instance.0.originate_default = originate_default;
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
                v3_instance.0.originate_default = originate_default;
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

    /// `use_ospf`: the default route comes over OSPF then; one from DHCP would be
    /// redistributed to the whole network, although it only works from this device.
    fn enable_dhcp_client(&mut self, if_name: AsciiString, use_ospf: bool) {
        self.dhcp_v_4_client
            .entry((if_name,))
            .or_insert(IpDhcpClientCfg {
                interface: Default::default(),
                add_default_route: if use_ospf { YesNo::No } else { YesNo::Yes },
                comment: None,
                // RouterOS reports the distance only with a default route, can't clear it and
                // reads the hidden one as its default 1 (mikrotik-model `default=1`)
                default_route_distance: if use_ospf { 1 } else { 10 },
                // RouterOS's default: without the client ID the DHCP server may see a new
                // client and hand out a different address, cutting off the management path
                dhcp_options: [
                    IpDhcpClientDhcpOptions::Hostname,
                    IpDhcpClientDhcpOptions::Clientid,
                ]
                .into(),
                script: None,
                use_peer_dns: true,
                use_peer_ntp: false,
            });
    }

    fn setup_ip_addresses<N: NameGenerator>(&mut self, l2: &L2Setup<N>) -> Result<(), SetupError> {
        // A device NetBox gives no address at all and no `dhcp-client` tag is pure L2
        // (a radio bridge): it is only reachable through an address from DHCP, so its
        // bridges get a client - under whatever name the bridge is generated with.
        let pure_l2 = l2
            .logical_interfaces()
            .all(|logical| logical.addresses.is_empty() && !logical.enable_dhcp_client);
        if pure_l2 {
            for bridge in &l2.simple_bridges {
                self.enable_dhcp_client(bridge.name.clone(), false);
            }
        }
        for logical in l2.logical_interfaces() {
            let if_name = logical.if_name.clone();
            let ips = logical
                .addresses
                .iter()
                .filter_map(|ip| ip.net().map(|a| (a, ip.clone())))
                .collect::<Vec<_>>();
            if logical.nat {
                self.masquerade(&if_name);
            }
            if ips.is_empty() {
                if logical.enable_dhcp_client {
                    self.enable_dhcp_client(if_name, logical.use_ospf);
                }
            } else {
                let dhcp_server = logical.enable_dhcp_server;
                for (ip_idx, (ip, ip_address)) in ips.iter().enumerate() {
                    self.set_ip_address(*ip, if_name.clone());
                    // a client network (`dhcp`) gets its IPv6 /64 announced for SLAAC
                    if let (true, IpNet::V6(v6_net)) = (dhcp_server, ip)
                        && v6_net.prefix_len() == 64
                        && let Some(address) = self.ipv_6_address.get_mut(v6_net)
                    {
                        address.0.advertise = true;
                    }
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
                                if !network.dns_server.contains(&ip.addr()) {
                                    network.dns_server = network
                                        .dns_server
                                        .iter()
                                        .copied()
                                        .chain([ip.addr()])
                                        .collect();
                                }
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
    /// DNS of the client networks. With DNS servers from the config context they get these:
    /// the IPv4 ones by DHCP, the IPv6 ones by router advertisement (RDNSS), in their
    /// order; the device asks them too and resolves for nobody. Without, the DHCP networks
    /// name the device itself, so it answers them (not from a `nat` uplink); one without a
    /// DHCP client learns no DNS server and asks the NAT routers (NetBox tag `nat`).
    fn setup_dns(&mut self, device: &DeviceAccess) {
        let servers = device.dns_servers();
        let nd = &mut self
            .ipv_6_nd
            .entry(b"all".into())
            .or_insert_with(|| Ipv6NdByInterface(Ipv6NdCfg::new(b"all".into())))
            .0;
        if !servers.is_empty() {
            let v4 = servers
                .iter()
                .filter_map(|ip| match ip {
                    IpAddr::V4(ip) => Some(*ip),
                    IpAddr::V6(_) => None,
                })
                .collect::<Box<[_]>>();
            let v6 = servers
                .iter()
                .filter_map(|ip| match ip {
                    IpAddr::V6(ip) => Some(*ip),
                    IpAddr::V4(_) => None,
                })
                .collect::<Box<[_]>>();
            // only where there are client networks to announce them to
            let client_networks = !self.dhcp_v_4_server_network.is_empty()
                || self.ipv_6_address.values().any(|address| address.0.advertise);
            if client_networks && !v6.is_empty() {
                nd.dns = Some(v6);
                nd.advertise_dns = Some(Ipv6NdAdvertiseDns::Yes);
            }
            for network in self.dhcp_v_4_server_network.values_mut() {
                network.0.dns_server = v4.clone();
            }
            self.dns.allow_remote_requests = false;
            self.dns.servers = servers.into();
            return;
        }
        self.dns.allow_remote_requests = !self.dhcp_v_4_server_network.is_empty();
        if !self.dns.allow_remote_requests {
            return;
        }
        // the resolver answers the device's own networks, not the uplink's
        let uplinks = self
            .ip_firewall_nat
            .values()
            .filter_map(|nat| nat.out_interface.clone())
            .collect::<Vec<_>>();
        for uplink in uplinks {
            self.close_dns(&uplink);
        }
        let has_uplink = device
            .interfaces()
            .iter()
            .any(|interface| interface.is_enable_dhcp_client());
        if !has_uplink {
            self.dns.servers = device
                .topology()
                .list_devices()
                .filter(|other| other.id() != device.id())
                .filter(|other| other.interfaces().iter().any(|i| i.is_nat()))
                .filter_map(|other| other.loopback_ip().and_then(|ip| ip.addr()))
                .collect();
        }
    }
    /// Masquerades what leaves through the uplink `if_name` (NetBox tag `nat`).
    fn masquerade(&mut self, if_name: &AsciiString) {
        self.ip_firewall_nat.insert(
            (b"srcnat".into(), Some(if_name.clone())),
            IpFirewallNatCfg {
                action: IpFirewallNatAction::Masquerade,
                chain: b"srcnat".into(),
                comment: None,
                disabled: false,
                dst_address: None,
                dst_port: None,
                in_interface: None,
                in_interface_list: None,
                jump_target: None,
                out_interface: Some(if_name.clone()),
                out_interface_list: None,
                protocol: None,
                src_address: None,
                to_addresses: None,
                to_ports: None,
            },
        );
    }
    /// Keeps the DNS resolver closed to the uplink `if_name`.
    fn close_dns(&mut self, if_name: &AsciiString) {
        for protocol in [b"udp", b"tcp"] {
            let protocol = AsciiString::from(protocol.as_slice());
            let port = AsciiString::from(b"53".as_slice());
            self.ip_firewall_filter.insert(
                (
                    b"input".into(),
                    Some(if_name.clone()),
                    Some(protocol.clone()),
                    Some(port.clone()),
                ),
                IpFirewallFilterCfg {
                    action: IpFirewallFilterAction::Drop,
                    chain: b"input".into(),
                    comment: None,
                    connection_state: None,
                    disabled: false,
                    dst_address: None,
                    dst_port: Some(port),
                    in_interface: Some(if_name.clone()),
                    in_interface_list: None,
                    jump_target: None,
                    out_interface: None,
                    out_interface_list: None,
                    protocol: Some(protocol),
                    src_address: None,
                },
            );
        }
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
                .map(|port| (port.default_name, port.data))
                .collect(),
        }
    }
    /// Configures every legacy wireless radio as NetBox describes it. One with an RF
    /// role is an end of a point-to-point bridge: it gets the board's whole radio
    /// profile (`hw_facts::wireless_bridge_port`), the AP side running plain Nv2, the
    /// station side also falling back to Nstreme/802.11 (as the production links
    /// do), with the wireless link's SSID, key and regulatory settings. One without
    /// is not managed as a radio and keeps its `current` configuration (e.g. a CAP
    /// of CAPsMAN), only named after its interface if the L2 setup uses it.
    fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WirelessDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let profiles = self.wireless.clone();
        // a radio the board doesn't have is a card in its miniPCIe slot (`wlan2` of an RB912)
        for port in current.iter().flat_map(|current| current.wireless.iter()) {
            self.wireless
                .insert(port.default_name.clone(), port.data.clone());
        }
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::Wlan(_)) = interface.external_port() else {
                continue;
            };
            let key = Some(AsciiString::from(port.to_string()));
            let wlan = self
                .wireless
                .get_mut(&key)
                .ok_or(SetupError::PortNotFound(port))?;
            if let Some(radio) = interface.radio() {
                let profile = profiles
                    .get(&key)
                    .filter(|profile| profile.antenna_gain.is_some())
                    .ok_or(SetupError::NoRadioProfile(port))?;
                *wlan = profile.clone();
                configure_bridge_radio(wlan, device, port, radio)?;
            }
            if let Some((name, _)) = l2.placement_of_port(port) {
                wlan.name = name.clone();
            }
            let mtu = port_mtu(&interface)?;
            let l2mtu = profiles.get(&key).and_then(|p| p.l_2_mtu).or(wlan.l_2_mtu);
            wlan.mtu = Some(mtu.unwrap_or(DEFAULT_MTU));
            wlan.l_2_mtu = l2mtu_for(l2mtu, mtu);
        }
        self.setup_legacy_cap(device, l2, current)
    }
    /// A CAP of the legacy CAPsMAN (`Topology::is_legacy_cap`) with every radio NetBox has,
    /// finding the controller at its loopback like a CAP of `/interface/wifi`. The CAP
    /// forwards the clients' traffic itself (`local-forwarding`, `CapsManDeviceDataTarget`)
    /// into `bridge`, the one carrying the WLANs' VLANs and their VXLAN, like a CAP of
    /// `/interface/wifi`. An AP whose radios NetBox doesn't have keeps its CAP as it is (one
    /// of a CAPsMAN NetBox doesn't know of), any other device has it off.
    fn setup_legacy_cap<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WirelessDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let Some(group) = device.legacy_cap_of() else {
            self.wireless_cap = match (device.wlan_ap_of(), current) {
                (Some(_), Some(current)) => current.wireless_cap.clone(),
                _ => Default::default(),
            };
            return Ok(());
        };
        let controller = group
            .controller()
            .and_then(|controller| controller.loopback_ip())
            .and_then(|ip| ip.addr())
            .ok_or_else(|| SetupError::MissingController(device.name().into()))?;
        let cap = &mut self.wireless_cap;
        *cap = Default::default();
        cap.enabled = true;
        cap.caps_man_addresses = [controller].into();
        cap.bridge = group
            .wlan()
            .find_map(|wlan| l2.bridge_of_wlan(&wlan))
            .map_or(HasNone::NoneValue, |bridge| HasNone::Value(bridge.clone()));
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::Wlan(_)) = interface.external_port() else {
                continue;
            };
            let name = match l2.placement_of_port(port) {
                Some((name, _)) => name.clone(),
                None => AsciiString::from(port.to_string()),
            };
            cap.interfaces.insert(name);
            // `wireless` installs its radios disabled (a wAP ac coming from `wifi-qcom-ac`)
            if let Some(wlan) = self.wireless.get_mut(&Some(AsciiString::from(port.to_string()))) {
                wlan.disabled = false;
            }
        }
        Ok(())
    }
}

/// Sets `wlan`, a 5 GHz bridge radio profile, up as the `radio` end of its link.
fn configure_bridge_radio(
    wlan: &mut InterfaceWirelessCfg,
    device: &DeviceAccess,
    port: PhysicalPortId,
    radio: &Radio,
) -> Result<(), SetupError> {
    let link = radio
        .link
        .as_ref()
        .ok_or(SetupError::MissingWirelessLink(port))?;
    let (mode, protocol) = match radio.role {
        RadioRole::Ap => (InterfaceWirelessMode::Bridge, Nv2),
        RadioRole::Station => (InterfaceWirelessMode::StationBridge, Nv2Nstreme80211),
    };
    wlan.mode = Some(mode);
    wlan.wireless_protocol = Some(protocol);
    // a station scans for its AP
    if radio.role == RadioRole::Ap {
        wlan.frequency = Some(radio.frequency.map_or(Auto::Auto, Auto::Value));
    }
    if let Some(width) = radio.channel_width {
        wlan.channel_width = Some(wireless_channel_width(width, radio.role)?);
    }
    wlan.ssid = Some(link.ssid.as_ref().into());
    if let Some(key) = link_key(link)? {
        wlan.nv_2_security = Some(EnabledDisabled::Enabled);
        wlan.nv_2_preshared_key = Some(key.into());
    }
    wlan.country = Some(match &link.country {
        Some(country) => country.as_ref().into(),
        None => country_settings(device)?.0.into(),
    });
    if let Some(mode) = &link.frequency_mode {
        wlan.frequency_mode = Some(
            match InterfaceWirelessFrequencyMode::parse_ros(mode.as_bytes()) {
                ParseRosValueResult::Value(mode) => mode,
                _ => return Err(SetupError::InvalidFrequencyMode(mode.clone())),
            },
        );
    }
    if let Some(power) = radio.tx_power {
        wlan.tx_power_mode = Some(InterfaceWirelessTxPowerMode::AllRatesFixed);
        wlan.tx_power = Some(power);
    }
    Ok(())
}

/// The key of an encrypted link, `None` for an open one.
fn link_key(link: &RadioLink) -> Result<Option<&str>, SetupError> {
    if !link.encrypted {
        return Ok(None);
    }
    link.psk
        .as_deref()
        .map(Some)
        .ok_or_else(|| SetupError::MissingPsk(link.ssid.clone()))
}

/// RouterOS's `/interface/wireless` country and `/interface/w60g` region for the
/// country of the device's site (the ISO 3166 code of its topmost region).
fn country_settings(
    device: &DeviceAccess,
) -> Result<(&'static str, InterfaceW60GRegion), SetupError> {
    use InterfaceW60GRegion::Eu;
    let code = device
        .country()
        .ok_or_else(|| SetupError::MissingCountry(device.name().into()))?;
    Ok(match code.to_ascii_lowercase().as_str() {
        "ch" => ("switzerland", Eu),
        "li" => ("liechtenstein", Eu),
        "de" => ("germany", Eu),
        "at" => ("austria", Eu),
        "fr" => ("france", Eu),
        "it" => ("italy", Eu),
        _ => return Err(SetupError::UnknownCountry(code.into())),
    })
}

/// The channel width for `width` MHz with the control channel first, as on the AP
/// side; a station follows the AP's layout (`X`), whatever it is.
fn wireless_channel_width(
    width: u32,
    role: RadioRole,
) -> Result<InterfaceWirelessChannelWidth, SetupError> {
    use InterfaceWirelessChannelWidth::*;
    Ok(match (width, role) {
        (5, _) => _5Mhz,
        (10, _) => _10Mhz,
        (20, _) => _20Mhz,
        (40, RadioRole::Ap) => _2040MhzCe,
        (40, RadioRole::Station) => _2040MhzXx,
        (80, RadioRole::Ap) => _204080MhzCeee,
        (80, RadioRole::Station) => _204080MhzXxxx,
        (160, RadioRole::Ap) => _204080160MhzCeeeeeee,
        (160, RadioRole::Station) => _204080160MhzXxxxxxxx,
        _ => return Err(SetupError::UnsupportedChannelWidth(width)),
    })
}

mikrotik_model!(
    name = W60GDeviceData,
    detect = new,
    fields(radio(by_key(path = "interface/w60g", key = defaultName)),),
);

impl W60GDeviceDataTarget {
    fn new(model: &[u8]) -> Self {
        Self {
            radio: build_w60g_ports(model)
                .into_iter()
                .map(|port| (port.default_name, port.data))
                .collect(),
        }
    }
    /// Like the legacy wireless radios: one with an RF role is a bridge end and gets
    /// the whole 60 GHz profile (`hw_facts::w60g_port`); its stations - the far ends,
    /// as dynamic `/interface/w60g/station` interfaces on the AP side - join the
    /// bridge the radio is in (`put-stations-in-bridge`), the wireless link's PSK is
    /// the password. One without keeps its `current` configuration.
    fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&W60GDeviceDataCurrent>,
    ) -> Result<(), SetupError> {
        let profiles = self.radio.clone();
        for port in current.iter().flat_map(|current| current.radio.iter()) {
            if let Some(target) = self.radio.get_mut(&port.default_name) {
                *target = port.data.clone();
            }
        }
        for interface in device.interfaces() {
            let Some(port @ PhysicalPortId::W60g(_)) = interface.external_port() else {
                continue;
            };
            let key = AsciiString::from(port.to_string());
            let radio_port = self
                .radio
                .get_mut(&key)
                .ok_or(SetupError::PortNotFound(port))?;
            let placement = l2.placement_of_port(port);
            if let Some(radio) = interface.radio() {
                let link = radio
                    .link
                    .as_ref()
                    .ok_or(SetupError::MissingWirelessLink(port))?;
                *radio_port = profiles[&key].clone();
                radio_port.mode = Some(match radio.role {
                    RadioRole::Ap => InterfaceW60GMode::Bridge,
                    RadioRole::Station => InterfaceW60GMode::StationBridge,
                });
                if radio.role == RadioRole::Ap {
                    radio_port.frequency = Some(radio.frequency.map_or(Auto::Auto, Auto::Value));
                    radio_port.isolate_stations = Some(true);
                }
                radio_port.put_stations_in_bridge =
                    placement.and_then(|(_, bridge)| bridge.cloned());
                radio_port.ssid = Some(link.ssid.as_ref().into());
                radio_port.password = link_key(link)?.map(Into::into);
                radio_port.region = Some(match &link.country {
                    Some(region) => match InterfaceW60GRegion::parse_ros(region.as_bytes()) {
                        ParseRosValueResult::Value(region) => region,
                        _ => return Err(SetupError::UnknownCountry(region.clone())),
                    },
                    None => country_settings(device)?.1,
                });
            }
            if let Some((name, _)) = placement {
                radio_port.name = name.clone();
            }
        }
        Ok(())
    }
}
mikrotik_model!(
    name = WifiDeviceData,
    detect = new,
    fields(
        cap(single = "interface/wifi/cap"),
        capsman(single = "interface/wifi/capsman"),
        wifi(by_key(path = "interface/wifi", key = defaultName)),
        datapath(by_key(path = "interface/wifi/datapath", key = name)),
        configuration(by_key(path = "interface/wifi/configuration", key = name)),
        certificate(by_key(path = "certificate", key = name)),
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
    fn new(_model: &[u8]) -> Self {
        Self {
            provisioning: Default::default(),
            datapath: Default::default(),
            cap: Default::default(),
            capsman: Default::default(),
            wifi: Default::default(),
            configuration: Default::default(),
            certificate: Default::default(),
        }
    }
    /// The native WiFi stack (`/interface/wifi`) as a Wireless LAN Group in NetBox describes
    /// it. Its controller runs CAPsMAN with a configuration per WLAN (SSID, country of the
    /// controller's site, WPA2/WPA3 PSK or OWE); the first WLAN is the master interface of
    /// every radio, the others virtual ones. An AP of the group is a CAP that finds the
    /// controller at its loopback (routed, no L2 to the controller needed) and forwards
    /// locally: a radio NetBox puts into a bridge gets a datapath to that bridge, which
    /// RouterOS adds the radio and its virtual interfaces to (so `setup_l2` leaves out its
    /// bridge port). A radio without a bridge of its own goes into the VLAN of the group's
    /// WLANs, on the bridge carrying it (`bridge-caps`, with the VXLAN to the controller):
    /// through a datapath with that `vlan-id` where the radio package tags
    /// (`RadioPackage::tags_vlans`), else as an access port of the bridge (`setup_l2`). The
    /// CAP sets the VLAN, not the controller, which doesn't know its CAPs' boards; so a radio
    /// has one VLAN, and WLANs of a group with different VLANs are refused. Radios of a
    /// device that is no CAP keep their current configuration. Certificates stay, except
    /// the ones CAPsMAN generated (`capsman_certificates`) on a device that is neither
    /// controller nor CAP anymore.
    fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        current: Option<&WifiDeviceDataCurrent>,
        capsman_certificates: &[AsciiString],
        radio_package: Option<RadioPackage>,
        bound_radios: &[AsciiString],
    ) -> Result<Vec<ApplyWarning>, SetupError> {
        let mut warnings = Vec::new();
        // without a package of `/interface/wifi`, its radios are what one left (a wAP ac on
        // `wireless` now): inactive, but keeping their names, which the `wlan<N>` need
        let drivers = radio_package.is_some_and(RadioPackage::is_wifi);
        for port in current.iter().flat_map(|current| current.wifi.iter()) {
            let mut data = port.data.clone();
            if let (false, Some(default_name)) = (drivers, &port.default_name) {
                data.name = default_name.clone();
            }
            self.wifi.insert(port.default_name.clone(), data);
        }
        let in_wlan = device.wifi_cap_of().is_some() || device.wlan_controller_of().is_some();
        for certificate in current.iter().flat_map(|current| current.certificate.iter()) {
            if in_wlan || !capsman_certificates.contains(&certificate.0.name) {
                self.certificate
                    .insert(certificate.0.name.clone(), certificate.clone());
            }
        }
        if let Some(group) = device.wifi_cap_of() {
            let controller = group
                .controller()
                .and_then(|controller| controller.loopback_ip())
                .and_then(|ip| ip.addr())
                .ok_or_else(|| SetupError::MissingController(device.name().into()))?;
            let cap = &mut self.cap;
            cap.enabled = YesNo::Yes;
            cap.caps_man_addresses = [controller].into();
            let wlan_vlans = wlan_vlans(&group);
            let wlan_bridge = group.wlan().find_map(|wlan| l2.bridge_of_wlan(&wlan));
            let tags = radio_package.is_some_and(RadioPackage::tags_vlans);
            // an AP without radios in NetBox (a controller also marked as AP) has none to tag
            let has_radios = device
                .interfaces()
                .iter()
                .any(|interface| matches!(interface.external_port(), Some(PhysicalPortId::Wifi(_))));
            if wlan_vlans == WlanVlans::PerWlan && !tags && has_radios {
                return Err(SetupError::WlanVlansNotTagged(device.name().into()));
            }
            // a configuration per datapath (`None`: none, an access port), the first `capsman`
            let mut cap_configurations: Vec<(Option<AsciiString>, AsciiString)> = Vec::new();
            for interface in device.interfaces() {
                let Some(port @ PhysicalPortId::Wifi(_)) = interface.external_port() else {
                    continue;
                };
                let default_name = AsciiString::from(port.to_string());
                let radio = self
                    .wifi
                    .get_mut(&Some(default_name.clone()))
                    .ok_or(SetupError::PortNotFound(port))?;
                let placement = l2.placement_of_port(port);
                // with a VLAN per WLAN, the controller's configurations have them
                let vlan = match (placement.and_then(|(_, bridge)| bridge), wlan_bridge, wlan_vlans) {
                    (None, Some(_), WlanVlans::One(Some(vlan))) if tags => Some(vlan),
                    _ => None,
                };
                let bridge = match placement {
                    None => None,
                    Some((_, Some(bridge))) => Some(bridge),
                    // an access port of the bridge, which `setup_l2` adds
                    Some((_, None)) if !tags => None,
                    Some((_, None)) => wlan_bridge,
                };
                if let Some(bridge) = bridge {
                    let datapath = &mut self.datapath.entry(bridge.clone()).or_default().0;
                    datapath.name = bridge.clone();
                    datapath.bridge = Some(HasNone::Value(bridge.clone()));
                    datapath.vlan_id = vlan.map(HasNone::Value);
                    datapath.disabled = false;
                    // virtual interfaces of the CAP use the one of the first radio
                    self.cap.slaves_datapath.get_or_insert_with(|| bridge.clone());
                }
                let configuration_name = match cap_configurations
                    .iter()
                    .find(|(datapath, _)| datapath.as_ref() == bridge)
                {
                    Some((_, name)) => name.clone(),
                    None => {
                        let name = match (cap_configurations.is_empty(), bridge) {
                            (true, _) => AsciiString::from("capsman"),
                            (false, Some(bridge)) => format!("capsman-{bridge}").as_str().into(),
                            (false, None) => AsciiString::from("capsman-local"),
                        };
                        cap_configurations.push((bridge.cloned(), name.clone()));
                        let configuration =
                            &mut self.configuration.entry(name.clone()).or_default().0;
                        configuration.name = name.clone();
                        configuration.disabled = false;
                        configuration.manager = Some(WifiManagement::Capsman);
                        configuration.datapath = bridge.cloned();
                        // reported with the datapath's values, which are set to the same
                        configuration.datapath_bridge = bridge.cloned();
                        configuration.datapath_vlan_id = vlan.map(HasNone::Value);
                        name
                    }
                };
                // A radio CAPsMAN has bound is dynamic and RouterOS refuses any change to it
                // (7.23.8, also with the CAP off): it stays as it is, a different configuration
                // is a warning.
                if bound_radios.contains(&default_name) {
                    if radio.configuration.as_ref() != Some(&configuration_name) {
                        warnings.push(ApplyWarning::RadioBound(RadioBound {
                            radio: default_name.to_string().into(),
                            configuration: radio.configuration.as_ref().map(|c| c.to_string().into()),
                            expected: configuration_name.to_string().into(),
                        }));
                    }
                    continue;
                }
                // under the name L2 gives it; the configuration has it all, which the radio
                // reports as its own
                radio.name = default_name;
                radio.configuration = Some(configuration_name);
                radio.configuration_manager = Some(WifiManagement::Capsman);
                radio.datapath = None;
                radio.datapath_bridge = bridge.cloned();
                radio.datapath_vlan_id = vlan.map(HasNone::Value);
            }
        }
        // what a bound radio uses stays, and with it its datapath
        for radio in current.iter().flat_map(|current| current.wifi.iter()) {
            let bound = radio
                .default_name
                .as_ref()
                .is_some_and(|name| bound_radios.contains(name));
            let Some(name) = radio.data.configuration.as_ref().filter(|_| bound) else {
                continue;
            };
            let Some(used) = current
                .iter()
                .flat_map(|current| current.configuration.iter())
                .find(|configuration| &configuration.0.name == name)
            else {
                continue;
            };
            let configuration = self
                .configuration
                .entry(name.clone())
                .or_insert_with(|| used.clone());
            if let Some(datapath) = configuration.0.datapath.clone()
                && let Some(used) = current
                    .iter()
                    .flat_map(|current| current.datapath.iter())
                    .find(|used| used.0.name == datapath)
            {
                self.datapath.entry(datapath).or_insert_with(|| used.clone());
            }
        }
        if let Some(group) = device.wlan_controller_of() {
            let capsman = &mut self.capsman;
            capsman.enabled = YesNo::Yes;
            // RouterOS's defaults, which it reports only once set
            capsman.interfaces = Some([HasNone::Value("all".into())].into());
            capsman.upgrade_policy = Some(InterfaceWifiCapsmanUpgradePolicy::SuggestSameVersion);
            capsman.require_peer_certificate = Some(false);
            let country = wifi_country(device)?;
            let per_wlan = wlan_vlans(&group) == WlanVlans::PerWlan;
            let mut names = Vec::new();
            for wlan in group.wlan() {
                let ssid = wlan.ssid().unwrap_or_default();
                let name = wlan_configuration_name(ssid);
                let configuration = &mut self.configuration.entry(name.clone()).or_default().0;
                configuration.name = name.clone();
                configuration.disabled = false;
                configuration.ssid = Some(ssid.into());
                configuration.country = Some(country);
                // the CAP's datapath gives the bridge, this the VLAN in it
                configuration.datapath_vlan_id = wlan
                    .vlan()
                    .and_then(|vlan| vlan.vlan_id())
                    .filter(|_| per_wlan)
                    .map(HasNone::Value);
                match wlan.auth() {
                    Some(WlanAuth::Wpa(settings)) => {
                        configuration.security_authentication_types =
                            [WifiAuthenticationTypes::Wpa2Psk, WifiAuthenticationTypes::Wpa3Psk]
                                .into();
                        configuration.security_passphrase = Some(settings.psk().into());
                    }
                    Some(WlanAuth::Open(settings)) if settings.use_owe => {
                        configuration.security_authentication_types =
                            [WifiAuthenticationTypes::Owe].into();
                    }
                    Some(WlanAuth::Open(_)) | None => {}
                }
                names.push(name);
            }
            if let Some((master, slaves)) = names.split_first() {
                let provisioning = self.provisioning.entry(Default::default()).or_default();
                provisioning.action = InterfaceWifiProvisioningAction::CreateDynamicEnabled;
                provisioning.disabled = false;
                provisioning.master_configuration = Some(master.clone());
                provisioning.slave_configurations =
                    (!slaves.is_empty()).then(|| slaves.iter().cloned().collect());
            }
        }
        Ok(warnings)
    }
}

mikrotik_model!(
    name = CapsManDeviceData,
    detect = new,
    fields(
        manager(single = "caps-man/manager"),
        security(by_key(path = "caps-man/security", key = name)),
        datapath(by_key(path = "caps-man/datapath", key = name)),
        configuration(by_key(path = "caps-man/configuration", key = name)),
        provisioning(by_id(
            path = "caps-man/provisioning",
            keys(
                radio_mac,
                identity_regexp,
                common_name_regexp,
                ip_address_ranges,
                hw_supported_modes
            )
        )),
    ),
);

impl CapsManDeviceDataTarget {
    fn new(_model: &[u8]) -> Self {
        Self {
            manager: Default::default(),
            security: Default::default(),
            datapath: Default::default(),
            configuration: Default::default(),
            provisioning: Default::default(),
        }
    }
    /// The legacy CAPsMAN (`/caps-man`, in `wireless`) of a controller whose Wireless LAN
    /// Group has CAPs of it (`Topology::is_legacy_cap`), next to the `/interface/wifi` one:
    /// a configuration per WLAN (named like that one's, SSID, country, WPA2 PSK), the first
    /// WLAN the master of every radio, the others virtual ones. The CAPs forward the clients'
    /// traffic themselves, into their bridge (`WirelessDeviceDataTarget::setup_legacy_cap`)
    /// in the WLAN's VLAN, from where the VXLAN carries it on. Only read where NetBox asks
    /// for it (`DeviceCfg::fetch`).
    fn generate_from(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        let Some(group) = device
            .wlan_controller_of()
            .filter(WlanGroupAccess::has_legacy_caps)
        else {
            return Ok(());
        };
        self.manager.enabled = true;
        let country = match CapsManWifiCountry::parse_ros(country_settings(device)?.0.as_bytes()) {
            ParseRosValueResult::Value(country) => country,
            _ => return Err(SetupError::UnknownCountry(country_settings(device)?.0.into())),
        };
        let mut names = Vec::new();
        for wlan in group.wlan() {
            let ssid = wlan.ssid().unwrap_or_default();
            let name = wlan_configuration_name(ssid);
            let configuration = &mut self.configuration.entry(name.clone()).or_default().0;
            configuration.name = name.clone();
            configuration.mode = Some(CapsManConfigurationMode::Ap);
            configuration.ssid = Some(ssid.into());
            configuration.country = Some(country);
            configuration.datapath = Some(name.clone());
            if let Some(WlanAuth::Wpa(settings)) = wlan.auth() {
                let security = &mut self.security.entry(name.clone()).or_default().0;
                security.name = name.clone();
                security.authentication_types = [CapsAuthenticationTypes::Wpa2Psk].into();
                security.encryption = [CapsEncryption::AesCcm].into();
                security.passphrase = Some(settings.psk().into());
                configuration.security = Some(name.clone());
            }
            let datapath = &mut self.datapath.entry(name.clone()).or_default().0;
            datapath.name = name.clone();
            datapath.local_forwarding = Some(true);
            datapath.client_to_client_forwarding = Some(true);
            if let Some(vlan) = wlan.vlan().and_then(|vlan| vlan.vlan_id()) {
                datapath.vlan_mode = Some(CapsManDatapathVlanMode::UseTag);
                datapath.vlan_id = Some(vlan);
            }
            names.push(name);
        }
        if let Some((master, slaves)) = names.split_first() {
            let provisioning = CapsManProvisioningCfg {
                action: HasNone::Value(CapsManProvisioningAction::CreateDynamicEnabled),
                master_configuration: master.clone(),
                slave_configurations: slaves.iter().cloned().collect(),
                ..Default::default()
            };
            self.provisioning.insert(
                (
                    provisioning.radio_mac.clone(),
                    provisioning.identity_regexp.clone(),
                    provisioning.common_name_regexp.clone(),
                    provisioning.ip_address_ranges.clone(),
                    provisioning.hw_supported_modes.clone(),
                ),
                provisioning,
            );
        }
        Ok(())
    }
}

/// The VLANs the WLANs of a group put their clients in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WlanVlans {
    /// one at most: the CAP sets it on its radios (or makes them access ports, `wifi-qcom-ac`)
    One(Option<u16>),
    /// several: the controller sets each WLAN's in its configuration, a CAP's radios tag them
    PerWlan,
}

fn wlan_vlans(group: &WlanGroupAccess) -> WlanVlans {
    let vlans = group
        .wlan()
        .filter_map(|wlan| wlan.vlan().and_then(|vlan| vlan.vlan_id()))
        .collect::<BTreeSet<_>>();
    match vlans.len() {
        0 | 1 => WlanVlans::One(vlans.first().copied()),
        _ => WlanVlans::PerWlan,
    }
}

/// The name of a WLAN's configuration (`/interface/wifi` and `/caps-man`): its SSID with
/// every character but ASCII letters, digits and `-` as `_`.
fn wlan_configuration_name(ssid: &str) -> AsciiString {
    ssid.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect::<String>()
        .into()
}

/// The package for the board's radios (`hw_facts::radio_package`) if NetBox uses them: a
/// radio with an RF role, in a bridge, with a VLAN or an address, or the device is an AP of a
/// Wireless LAN Group. A controller without radios of a group with CAPs of the legacy
/// CAPsMAN needs `wireless` for running it (`CapsManDeviceDataTarget`), and so does a board
/// whose radios also run with it (`hw_facts::runs_wireless`) where NetBox has `wlan<N>` ports.
/// A RouterOS update installs just that besides `routeros`.
pub fn needed_radio_package(device: &DeviceAccess, model: &[u8]) -> Option<RadioPackage> {
    let Some(mut package) = radio_package(model) else {
        return device
            .wlan_controller_of()
            .is_some_and(|group| group.has_legacy_caps())
            .then_some(RadioPackage::Wireless);
    };
    if runs_wireless(model)
        && device
            .interfaces()
            .iter()
            .any(|interface| matches!(interface.external_port(), Some(PhysicalPortId::Wlan(_))))
    {
        package = RadioPackage::Wireless;
    }
    let uses_radio = device.interfaces().iter().any(|interface| {
        matches!(
            interface.external_port(),
            Some(PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_) | PhysicalPortId::W60g(_))
        ) && (interface.radio().is_some()
            || interface.bridge().is_some()
            || !interface.ips().is_empty()
            || interface.untagged_vlan().is_some()
            || interface.tagged_vlans().next().is_some())
    });
    (uses_radio || device.wlan_ap_of().is_some()).then_some(package)
}

/// The RouterOS name of the device's country for `/interface/wifi` (capitalized, unlike the
/// legacy `/interface/wireless` one).
fn wifi_country(device: &DeviceAccess) -> Result<WifiCountry, SetupError> {
    let (name, _) = country_settings(device)?;
    let mut name = name.to_string();
    name[..1].make_ascii_uppercase();
    match WifiCountry::parse_ros(name.as_bytes()) {
        ParseRosValueResult::Value(country) => Ok(country),
        _ => Err(SetupError::UnknownCountry(name.into())),
    }
}
