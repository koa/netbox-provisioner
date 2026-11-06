use crate::{
    Error,
    device::ros::{
        hw_facts::{RadioPackage, build_ethernet_ports},
        l2::{L2Port, L2Setup, NameGenerator, SimpleBridgePort},
    },
    topology::{
        IpPrefixId, PhysicalPortId, access::device::DeviceAccess, management::ManagementTree,
    },
};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        CapsManProvisioningHwSupportedModes, InterfaceBridgeProtocolMode,
        InterfaceEthernetByDefaultName, InterfaceEthernetCfg, InterfaceListByName,
        InterfaceListCfg, InterfaceListMemberCfg, InterfaceVlanByName, InterfaceVlanCfg,
        InterfaceVxlanByName, InterfaceVxlanCfg, IpFirewallNatAction, IpFirewallNatCfg,
        IpNeighborDiscoverySettingsCfg, IpSshCfg, IpSshForwardingEnabled, ToolMacServerCfg,
        VlanFrameTypes, WifiChannelBand,
    },
    value,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::IpAddr,
};

mod addresses;
pub mod apply;
pub mod cfg;
pub(crate) mod dhcp;
pub mod fallback;
mod firewall;
pub mod graphql;
pub(crate) mod hw_facts;
pub mod l2;
mod mtu;
mod ospf;
mod poe;
pub mod progress;
pub(crate) mod radio;
mod services;
pub(crate) mod speed;
pub(crate) mod syslog;
#[cfg(test)]
mod test;
pub mod warning;
pub mod watchdog;

pub(crate) use dhcp::dhcp_pool_v4;
pub(crate) use mtu::DEFAULT_MTU;
use mtu::{l2mtu_for, port_mtu};
pub use radio::needed_radio_package;

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
        // one per interface; the provisioner's at the internet uplinks, the others stay
        // (`keep_dhcp_v6_clients`)
        dhcp_v6_client(by_key(path = "ipv6/dhcp-client", key = interface)),
        // the firewall's own entries carry `FIREWALL_PREFIX` in their comment, the others stay
        ipv6_firewall_address_list(by_id(
            path = "ipv6/firewall/address-list",
            keys(address, list),
            owned = "netbox-provisioner:"
        )),
        ipv6_firewall_filter(by_id(
            path = "ipv6/firewall/filter",
            keys(),
            owned = "netbox-provisioner:",
            ordered
        )),
        ip_firewall_filter(by_id(
            path = "ip/firewall/filter",
            keys(),
            owned = "netbox-provisioner:",
            ordered
        )),
        ip_firewall_nat(by_id(
            path = "ip/firewall/nat",
            keys(),
            owned = "netbox-provisioner:",
            ordered
        )),
        dns(single = "ip/dns"),
        ipv6_nd(by_key(path = "ipv6/nd", key = interface)),
        dhcp_v4_server(by_key(path = "ip/dhcp-server", key = name)),
        dhcp_v4_server_network(by_key(path = "ip/dhcp-server/network", key = address)),
        dhcp_v4_lease(by_id(path = "ip/dhcp-server/lease", keys(address))),
        ipv4_pool(by_key(path = "ip/pool", key = name)),
        socks(single = "ip/socks"),
        socks_users(by_key(path = "ip/socks/users", key = name)),
        // the proxy's own rules carry `FIREWALL_PREFIX` in their comment, the others stay
        socks_access(by_id(
            path = "ip/socks/access",
            keys(),
            owned = "netbox-provisioner:",
            ordered
        )),
        ip_service(by_key(path = "ip/service", key = name)),
        ssh(single = "ip/ssh"),
        ipv6_settings(single = "ipv6/settings"),
        mac_server(single = "tool/mac-server"),
        neighbor_discovery(single = "ip/neighbor/discovery-settings"),
        watchdog(single = "system/watchdog"),
        logging_action(by_key(path = "system/logging/action", key = name)),
        logging(by_id(path = "system/logging", keys(action, topics))),
    ),
);

mikrotik_model!(
    name = WirelessDeviceData,
    detect = new,
    fields(
        wireless_cap(single = "interface/wireless/cap"),
        wireless(by_key(path = "interface/wireless", key = defaultName)),
    ),
);

mikrotik_model!(
    name = W60GDeviceData,
    detect = new,
    fields(radio(by_key(path = "interface/w60g", key = name)),),
);

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

/// `current` as it is once the interfaces `target` renames have their new names: what refers
/// to them follows a rename on RouterOS, so a bridge port, a list member or a bridge VLAN on a
/// renamed interface stays as it is instead of looking like an entry on another interface.
/// Paired by the names from before, every port of a renamed set looked like another, and one
/// was set onto an interface another port still held ("device already added as bridge port").
/// `more` are renames besides the ethernet ports' (the radios'), old name to new.
fn following_renames(
    target: &BaseDeviceDataTarget,
    current: &BaseDeviceDataCurrent,
    more: &[(AsciiString, AsciiString)],
) -> BaseDeviceDataCurrent {
    let renames: HashMap<&AsciiString, &AsciiString> = current
        .ethernet
        .iter()
        .filter_map(|port| {
            let new = &target.ethernet.get(&port.default_name)?.name;
            (new != &port.data.name).then_some((&port.data.name, new))
        })
        .chain(more.iter().map(|(old, new)| (old, new)))
        .collect();
    let mut current = current.clone();
    if renames.is_empty() {
        return current;
    }
    let follow = |name: &mut AsciiString| {
        if let Some(new) = renames.get(name) {
            *name = (*new).clone();
        }
    };
    for port in current.bridge_port.iter_mut() {
        follow(&mut port.data.interface);
    }
    for member in current.interface_list_member.iter_mut() {
        follow(&mut member.data.interface);
    }
    for client in current.dhcp_v_6_client.iter_mut() {
        follow(&mut client.0.interface);
    }
    for vlan in current.bridge_vlan.iter_mut() {
        vlan.data.tagged = vlan
            .data
            .tagged
            .iter()
            .cloned()
            .map(|mut n| {
                follow(&mut n);
                n
            })
            .collect();
        vlan.data.untagged = vlan
            .data
            .untagged
            .iter()
            .cloned()
            .map(|mut n| {
                follow(&mut n);
                n
            })
            .collect();
    }
    current
}

/// A VLAN of a port and the bridge NetBox puts it in ([`SetupError::PortInMultipleBridges`]).
#[derive(Debug, Clone, PartialEq)]
pub struct VlanOnBridge {
    /// `None`: untagged, without a VLAN
    pub vlan: Option<u16>,
    pub vlan_name: Option<Box<str>>,
    /// the NetBox bridge; `None`: none, the device's default bridge
    pub bridge: Option<Box<str>>,
}

impl std::fmt::Display for VlanOnBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.vlan, &self.vlan_name) {
            (Some(id), Some(name)) => write!(f, "VLAN {id} ({name})")?,
            (Some(id), None) => write!(f, "VLAN {id}")?,
            (None, _) => f.write_str("untagged")?,
        }
        match &self.bridge {
            Some(bridge) => write!(f, " on {bridge}"),
            None => f.write_str(" without a bridge"),
        }
    }
}

pub(crate) const CAPS_BRIDGE_NAME: &[u8; 11] = b"bridge-caps";
/// The interface list of the ports to own devices and the free ports
/// ([`BaseDeviceDataTarget::setup_maintenance_ports`]): `/tool mac-server` allows mac-telnet
/// there, and the device discovers its neighbors there (`/ip/neighbor`).
pub(crate) const MAINTENANCE_LIST: &[u8; 11] = b"maintenance";
/// The interface list of the `nat` uplinks: firewall rules set up by hand refer to it
/// (router-faesigrund-keller's IPv6 filter rejects what comes in there).
pub(crate) const UPLINK_LIST: &[u8; 6] = b"uplink";

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
    #[error("DHCP server on {interface}: no NetBox prefix {ip} for its pool")]
    MissingPrefixOnIpAddress { ip: IpNet, interface: AsciiString },
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
        "Port {port} would be a member of more than one bridge, its VLANs are split across \
         them: {}",
        vlans.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    PortInMultipleBridges {
        port: AsciiString,
        /// each VLAN of the port where it is, in NetBox' order of bridges
        vlans: Vec<VlanOnBridge>,
    },
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
    #[error("{0} has a speed in NetBox, which only Ethernet ports run at")]
    SpeedNotSupported(Box<str>),
    #[error("{port} runs at {speed} in NetBox: its cable needs a type (DAC, fiber or copper)")]
    SpeedNeedsCableType { port: Box<str>, speed: String },
    #[error(
        "{port} runs at {speed} in NetBox, which RouterOS has no speed for on it with its cable"
    )]
    UnknownSpeed { port: Box<str>, speed: String },
    #[error("{port} runs at {speed} in NetBox, which the board doesn't offer on it")]
    SpeedNotOnPort { port: Box<str>, speed: String },
    #[error("{port} runs at {speed} in NetBox, the other end of its cable, {far_end}, at {}", far_speed.as_deref().unwrap_or("auto-negotiation"))]
    SpeedMismatch {
        port: Box<str>,
        speed: String,
        far_end: Box<str>,
        far_speed: Option<Box<str>>,
    },
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
        // `setup_syslog` adds the server
        let (logging_action, logging) = syslog::routeros_logging();
        Self {
            ethernet: ethernet_ports
                .into_iter()
                .map(|e| (e.default_name, e.data))
                .collect(),
            identity: Default::default(),
            bridge: Default::default(),
            bridge_port: Default::default(),
            ospf_area: Default::default(),
            interface_list_member: Default::default(),
            ipv_4_address: Default::default(),
            ipv_6_address: Default::default(),
            dhcp_v_6_client: Default::default(),
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
            dhcp_v_4_lease: Default::default(),
            ipv_4_pool: Default::default(),
            socks: Default::default(),
            socks_users: Default::default(),
            socks_access: Default::default(),
            ip_service: Default::default(),
            // any device may be the neighbor a new one is set up from: the provisioner reaches
            // the new device through an SSH tunnel to it (`device::bootstrap`)
            ssh: IpSshCfg {
                forwarding_enabled: IpSshForwardingEnabled::Local,
                ..Default::default()
            },
            // every device gets OSPFv3 and its IPv6 addresses from NetBox; with IPv6 off they
            // are invalid and OSPFv3 never comes up, while the diff of them looks applied
            ipv_6_settings: Default::default(),
            // the list stays empty without ports to own devices: no mac-telnet at all then,
            // rather than on every port (`setup_maintenance_ports`)
            mac_server: ToolMacServerCfg {
                allowed_interface_list: MAINTENANCE_LIST.into(),
            },
            // not on customer ports; left alone it names the factory list `LAN` an apply
            // removes, which turns discovery off (`*2000011`)
            neighbor_discovery: IpNeighborDiscoverySettingsCfg {
                discover_interface_list: MAINTENANCE_LIST.into(),
            },
            interface_list: [(
                MAINTENANCE_LIST.into(),
                InterfaceListByName(InterfaceListCfg {
                    comment: Some(
                        b"ports to own devices and free ports: mac-telnet and neighbor discovery"
                            .into(),
                    ),
                    exclude: Default::default(),
                    include: Default::default(),
                    name: MAINTENANCE_LIST.into(),
                }),
            )]
            .into(),
            // set by `setup_watchdog`
            watchdog: Default::default(),
            logging_action,
            logging,
        }
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
            if setup.snoops_multicast(bridge.switch_ports(), bridge.carries_radio(), false) {
                bridge_cfg.0.snoop_multicast();
            }
            // Without vlan filtering RouterOS accepts the vlan fields but never reports them,
            // so a value here would be a diff forever; mvrp is reported by some versions
            // (7.20), not by others (7.24), and keeps whatever it is.
            bridge_cfg.0.ether_type = None;
            bridge_cfg.0.frame_types = None;
            bridge_cfg.0.ingress_filtering = None;
            bridge_cfg.0.pvid = None;
            bridge_cfg.0.mvrp = None;
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
                    // a `/interface/wireless` or `/interface/w60g` port has no hardware
                    // offloading, and RouterOS doesn't report `hw` for it (seen on 7.20.8,
                    // `wlan60-1` of a wAP 60G on 7.23.8)
                    if matches!(port_id, PhysicalPortId::Wlan(_) | PhysicalPortId::W60g(_)) {
                        bridge_port.hw = None;
                    }
                }
            }
        }
        for bridge in setup.tagged_bridges.iter() {
            let bridge_cfg = self.bridge.entry(bridge.name.clone()).or_default();
            bridge_cfg.0.vlan_filtering = true;
            bridge_cfg.0.mvrp = Some(false);
            if setup.snoops_multicast(bridge.switch_ports(), bridge.carries_radio(), true) {
                bridge_cfg.0.snoop_multicast();
            }
            bridge_cfg.0.protocol_mode = InterfaceBridgeProtocolMode::Mstp;
            // A bridge whose only way to other bridges is one VXLAN (a CAP's `bridge-caps`;
            // its radios are clients) closes no loop, and STP over the VXLAN, a segment it
            // shares with every bridge of the WLAN group, blocked its port (ap-buero-1og,
            // 7.23.7: no client got an address until STP was off). Without STP RouterOS
            // doesn't report the STP settings, but `forward-reserved-addresses` (7.24.5). It
            // keeps them hidden and rejects an empty value (`forward-delay=""`, 7.24.5), so they
            // stay at RouterOS's defaults, which mikrotik-model reads for a hidden one.
            let uplinks = bridge
                .ports
                .values()
                .flat_map(|entry| &entry.ports)
                .filter_map(|port| match port {
                    L2Port::TaggedEthernet { name, port, .. }
                    | L2Port::UntaggedEthernet { name, port, .. }
                        if !matches!(port, PhysicalPortId::Wlan(_) | PhysicalPortId::Wifi(_)) =>
                    {
                        Some((name, false))
                    }
                    L2Port::VxLan { name, .. } => Some((name, true)),
                    _ => None,
                })
                .collect::<HashSet<_>>();
            if uplinks.len() == 1 && uplinks.iter().all(|(_, vxlan)| *vxlan) {
                let bridge_cfg = &mut bridge_cfg.0;
                bridge_cfg.protocol_mode = InterfaceBridgeProtocolMode::None;
                bridge_cfg.region_name = None;
                bridge_cfg.forward_reserved_addresses = Some(false);
            }
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

    /// the base config with a fresh [`L2Setup`] (tests; the service shares one with the radios)
    #[cfg(test)]
    pub fn generate_from<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        name_generator: N,
    ) -> Result<L2Setup<N>, SetupError> {
        self.set_name_and_loopback(device);

        let l2 = L2Setup::new(device, name_generator)?;
        self.setup_addressing(device, &l2)?;
        self.setup_ports(device, &l2, None)?;
        Ok(l2)
    }

    /// The steps before the radios (`DeviceCfg::generate_from` and the tests share them):
    /// addresses, DNS and OSPF.
    pub(super) fn setup_addressing<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
    ) -> Result<(), SetupError> {
        self.setup_ip_addresses(l2)?;
        self.setup_dns(device);
        self.setup_perimeter(device);
        self.setup_ospf(device, l2);
        Ok(())
    }

    /// The steps after the radios, whose ports `setup_l2` puts in the bridges: L2, the
    /// maintenance ports, MTU, speed, PoE and the watchdog (towards the device's own
    /// management tree; `generate_for` sets the provisioner's).
    pub(super) fn setup_ports<N: NameGenerator>(
        &mut self,
        device: &DeviceAccess,
        l2: &L2Setup<N>,
        radio_package: Option<RadioPackage>,
    ) -> Result<(), SetupError> {
        self.setup_l2(device, l2, radio_package)?;
        self.setup_maintenance_ports(device, l2);
        self.setup_mtu(device)?;
        self.setup_speed(device)?;
        self.setup_poe(device)?;
        self.setup_watchdog(device, &ManagementTree::of(device.topology(), None))
    }

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
    fn setup_maintenance_ports<N: NameGenerator>(
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
            if own && let Some((name, _)) = l2.placement_of_port(id) {
                members.insert(name.clone());
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

    /// The speed NetBox gives an Ethernet port, without auto-negotiation (`speed`).
    fn setup_speed(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        for port in device.interfaces() {
            let Some(speed) = speed::fixed_speed(&port)? else {
                continue;
            };
            let ethernet_port = match port.external_port() {
                Some(port_id) => self.get_ethernet_port(port_id)?,
                None => None,
            }
            .ok_or_else(|| SetupError::SpeedNotSupported(port.name().into()))?;
            if !ethernet_port.advertise.contains(&speed) {
                return Err(SetupError::SpeedNotOnPort {
                    port: port.name().into(),
                    speed: speed::label(port.speed().unwrap_or_default()),
                });
            }
            ethernet_port.auto_negotiation = false;
            ethernet_port.speed = Some(speed);
            // RouterOS shows no `advertise` without auto-negotiation (klr-sw-1 qsfpplus1-1)
            ethernet_port.advertise = Default::default();
        }
        Ok(())
    }

    fn set_name_and_loopback(&mut self, device: &DeviceAccess) {
        self.set_identity(device.name());
        if let Some(loopback_ip) = device.loopback_ip().and_then(|ip| ip.addr()) {
            self.set_loopback_ip(loopback_ip);
        }
    }

    /// Puts the uplink `if_name` (NetBox tag `internet` or `nat`) in the list `uplink`, which
    /// the perimeter firewall guards.
    fn add_to_uplinks(&mut self, if_name: &AsciiString) {
        let list = AsciiString::from(UPLINK_LIST);
        self.interface_list.entry(list.clone()).or_insert_with(|| {
            InterfaceListByName(InterfaceListCfg {
                comment: Some(b"interfaces to the internet: perimeter firewall".into()),
                exclude: Default::default(),
                include: Default::default(),
                name: list.clone(),
            })
        });
        self.interface_list_member.insert(
            (if_name.clone(), list.clone()),
            InterfaceListMemberCfg {
                comment: None,
                disabled: false,
                interface: if_name.clone(),
                list,
            },
        );
    }
    /// Masquerades what leaves through the uplink `if_name` (NetBox tag `nat`).
    fn masquerade(&mut self, if_name: &AsciiString) {
        self.ip_firewall_nat.push(IpFirewallNatCfg {
            action: IpFirewallNatAction::Masquerade,
            chain: b"srcnat".into(),
            comment: Some(firewall_comment(&["masquerade", &if_name.to_string()])),
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
        });
    }
}

/// What the generator's firewall entries begin their comment with: only those are the
/// provisioner's, the others on the device stay (the `owned` firewall menus of the model).
const FIREWALL_PREFIX: &str = "netbox-provisioner:";

/// The comment of a firewall entry: the prefix and what the entry is, unique in its menu, as
/// the order of the rules finds them by it (`move [find comment=…]`).
fn firewall_comment(parts: &[&str]) -> AsciiString {
    AsciiString::from(format!("{FIREWALL_PREFIX} {}", parts.join(" ")).as_str())
}
