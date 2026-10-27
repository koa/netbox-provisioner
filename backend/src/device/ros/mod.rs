use crate::{
    Error,
    device::ros::{
        hw_facts::{RadioPackage, build_ethernet_ports},
        l2::{L2Port, L2Setup, NameGenerator, SimpleBridgePort},
        warning::{ApiClientsOutside, ApplyWarning, PoeCut},
    },
    topology::{
        IpPrefixId, PhysicalPortId,
        access::{AccessTopology, device::DeviceAccess, interface::InterfaceAccess},
        management::ManagementTree,
    },
};
use dhcp::{pool_range, static_leases};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use mikrotik_model::{
    MacAddress, MikrotikDevice,
    ascii::{self, AsciiString},
    mikrotik_model,
    model::{
        CapsManProvisioningHwSupportedModes, InterfaceBridgeProtocolMode,
        InterfaceEthernetByDefaultName, InterfaceEthernetCfg, InterfaceEthernetPoeOut,
        InterfaceListByName, InterfaceListCfg, InterfaceListMemberCfg, InterfaceVlanByName,
        InterfaceVlanCfg, InterfaceVxlanByName, InterfaceVxlanCfg, IpAddressByAddress,
        IpAddressCfg, IpDhcpClientCfg, IpDhcpClientDhcpOptions, IpDhcpServerLeaseCfg,
        IpFirewallFilterAction,
        IpFirewallFilterCfg, IpFirewallNatAction, IpFirewallNatCfg,
        IpNeighborDiscoverySettingsCfg, IpSocksAuthMethod, IpServiceByName, IpSocksUsersByName, IpSocksUsersCfg,
        IpSshCfg, IpSshForwardingEnabled, Ipv6AddressByAddress, Ipv6AddressCfg,
        Ipv6NdAdvertiseDns, Ipv6NdByInterface, Ipv6NdCfg, RoutingOriginateDefault,
        RoutingOspfInstanceByName, RoutingOspfInstanceCfg, RoutingOspfInstanceVersion,
        RoutingRedistribute, ToolMacServerCfg, VlanFrameTypes, WifiChannelBand, YesNo,
    },
    value,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::IpAddr,
    time::Duration,
};

pub(crate) mod dhcp;
pub mod fallback;
pub mod graphql;
pub mod progress;
pub(crate) mod hw_facts;
pub mod l2;
pub(crate) mod radio;
pub(crate) mod speed;
pub(crate) mod syslog;
pub mod warning;
pub mod watchdog;
#[cfg(test)]
mod test;

pub(crate) use dhcp::dhcp_pool_v4;
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
        // the firewall's own entries carry `FIREWALL_PREFIX` in their comment, the others stay
        ipv6_firewall_address_list(by_id(
            path = "ipv6/firewall/address-list",
            keys(address, list),
            owned = "netbox-provisioner:"
        )),
        ipv6_firewall_filter(by_id(path = "ipv6/firewall/filter", keys(), owned = "netbox-provisioner:", ordered)),
        ip_firewall_filter(by_id(
            path = "ip/firewall/filter",
            keys(chain, in_interface, protocol, dst_port),
            owned = "netbox-provisioner:",
            ordered
        )),
        ip_firewall_nat(by_id(
            path = "ip/firewall/nat",
            keys(chain, out_interface),
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
    for vlan in current.bridge_vlan.iter_mut() {
        vlan.data.tagged = vlan.data.tagged.iter().cloned().map(|mut n| { follow(&mut n); n }).collect();
        vlan.data.untagged = vlan.data.untagged.iter().cloned().map(|mut n| { follow(&mut n); n }).collect();
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
    #[error("{port} runs at {speed} in NetBox, which RouterOS has no speed for on it with its cable")]
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

/// RouterOS's L3 MTU of a port NetBox gives none.
pub(crate) const DEFAULT_MTU: u16 = 1500;
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

/// What OSPF announces besides its interfaces' networks: connected and static routes, and BGP's,
/// which is configured by hand (on klr-sw-3, `examples/` 01 to 03) and reaches the rest of the
/// network only this way. Where there is no BGP it brings nothing. A default route doesn't go out
/// this way (CHR 7.24.5: a static `0.0.0.0/0` with `redistribute=static` is no external LSA), only
/// by `originate-default`.
fn ospf_redistribute() -> BTreeSet<RoutingRedistribute> {
    [RoutingRedistribute::Connected, RoutingRedistribute::Static, RoutingRedistribute::Bgp]
        .into_iter()
        .collect()
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
                    comment: Some(b"ports to own devices and free ports: mac-telnet and neighbor discovery".into()),
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
    /// `/ip/service`: RouterOS's services as the device has them (`current`), with telnet, ftp
    /// and www off (A2) and, with `api_networks`, the plain API only from those. Written as
    /// `address`, which RouterOS 7.24 still takes for `available-from`, unless the device
    /// reports `available-from` already: an open service reports neither.
    pub fn setup_services(&mut self, current: &[IpServiceByName], api_networks: Option<&[IpNet]>) {
        let api_networks = api_networks.filter(|networks| !networks.is_empty()).map(|networks| {
            let list = networks.iter().map(|net| net.trunc().to_string()).collect::<Vec<_>>();
            AsciiString::from(list.join(",").as_str())
        });
        self.ip_service = current.iter().map(|service| (service.name.clone(), service.data.clone())).collect();
        for (name, service) in &mut self.ip_service {
            match &**name {
                b"telnet" | b"ftp" | b"www" => service.disabled = true,
                b"api" => {
                    if let Some(networks) = &api_networks {
                        if service.available_from.is_some() {
                            service.available_from = Some(networks.clone());
                        } else {
                            service.address = Some(networks.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    /// The API restriction of [`Self::setup_services`] only where it locks none of `clients`
    /// (the API's open connections) out: otherwise the API stays as in `current`, with a
    /// warning naming those outside `networks`.
    pub fn keep_api_open_for(
        &mut self,
        current: &[IpServiceByName],
        clients: &[IpAddr],
        networks: &[IpNet],
    ) -> Option<ApplyWarning> {
        let outside = clients
            .iter()
            .filter(|client| !networks.iter().any(|net| net.contains(*client)))
            .map(|client| client.to_string().into())
            .collect::<Vec<Box<str>>>();
        if outside.is_empty() {
            return None;
        }
        let name = AsciiString::from("api");
        if let (Some(target), Some(current)) =
            (self.ip_service.get_mut(&name), current.iter().find(|service| service.name == name))
        {
            *target = current.data.clone();
        }
        Some(ApplyWarning::ApiClientsOutside(ApiClientsOutside { clients: outside }))
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
            // without it Android and ChromeOS got no IPv6 address in the WLAN behind
            // srv-router-berg (SLAAC only), a PC on the LAN did
            bridge_cfg.0.snoop_multicast();
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
            // as above: the WLAN's SLAAC clients got no IPv6 address without it
            bridge_cfg.0.snoop_multicast();
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
    fn setup_maintenance_ports<N: NameGenerator>(&mut self, device: &DeviceAccess, l2: &L2Setup<N>) {
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

    /// PoE out of every port NetBox has: `auto-on` with `poe_mode=pse`, `forced-on` if its
    /// `poe_type` is passive as well, else `off`. `auto-on` feeds only a load it detects, which
    /// a UISP Wave Nano, passive PoE only, isn't (lab-np1 `e08`: `waiting-for-load`, with
    /// `forced-on` `powered-on` at 23.5 V, 7.23.8). `forced-on` puts the voltage on the port
    /// without asking, so only where NetBox says the device at the other end takes it.
    fn setup_poe(&mut self, device: &DeviceAccess) -> Result<(), SetupError> {
        for port in device.interfaces() {
            if let Some(port_id) = port.external_port()
                && let Some(ethernet_port) = self.get_ethernet_port(port_id)?
            {
                // `generate_ethernet()` already leaves `poe_out` at `None` for
                // boards without PoE-out hardware (e.g. CHR, several switch
                // models); only override it here if the baseline already
                // established PoE capability, otherwise RouterOS rejects the
                // whole `/interface/ethernet/set` call with "bad parameter
                // poe-out" - breaking the rename bundled into the same call.
                if ethernet_port.poe_out.is_some() {
                    ethernet_port.poe_out = Some(match (port.enable_poe(), port.passive_poe()) {
                        (true, true) => InterfaceEthernetPoeOut::ForcedOn,
                        (true, false) => InterfaceEthernetPoeOut::AutoOn,
                        (false, _) => InterfaceEthernetPoeOut::Off,
                    });
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
                v2_instance.0.redistribute = ospf_redistribute();
                v2_instance.0.router_id = Some(router_id.to_string().into());
                v2_instance.0.originate_default = originate_default;
                v2_instance.0.version = RoutingOspfInstanceVersion::_2;
                let v3_instance = self
                    .ospf_instance
                    .entry(b"default-v3".into())
                    .or_insert(RoutingOspfInstanceByName(RoutingOspfInstanceCfg::default()));
                v3_instance.0.redistribute = ospf_redistribute();
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
                    // the pool comes from the IPv4 prefix; IPv6 clients take SLAAC
                    if let (true, IpNet::V4(v4)) = (dhcp_server, ip) {
                        let Some(prefix) = ip_address.prefix() else {
                            // a /32 (a loopback) has no address for a client: no DHCP server,
                            // as with an empty pool (finding `DhcpPoolEmpty`)
                            if v4.prefix_len() == 32 {
                                continue;
                            }
                            return Err(SetupError::MissingPrefixOnIpAddress {
                                ip: ip.trunc(),
                                interface: if_name.clone(),
                            });
                        };
                        let IpNet::V4(net) = prefix.prefix().ok_or(SetupError::MissingAddressOnPrefix {
                            prefix: prefix.id(),
                        })?
                        else {
                            continue;
                        };
                        let dhcp_ranges = dhcp_pool_v4(&prefix, net);
                        if dhcp_ranges.is_empty() {
                            continue;
                        }
                        // numbered among the IPv4 addresses only: an IPv6 address before it in
                        // NetBox (one created again first by an undo) doesn't rename the server
                        let v4_idx = ips[..ip_idx]
                            .iter()
                            .filter(|(ip, _)| matches!(ip, IpNet::V4(_)))
                            .count();
                        let suffix = if v4_idx == 0 {
                            if_name.to_string()
                        } else {
                            format!("{if_name}-{v4_idx}")
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
                        network.gateway.insert(v4.addr());
                        if !network.dns_server.contains(&v4.addr()) {
                            network.dns_server = network
                                .dns_server
                                .iter()
                                .copied()
                                .chain([v4.addr()])
                                .collect();
                        }
                        for lease in static_leases(&prefix) {
                            self.dhcp_v_4_lease.insert(
                                (IpAddr::V4(lease.address),),
                                IpDhcpServerLeaseCfg {
                                    address: IpAddr::V4(lease.address),
                                    mac_address: Some(lease.mac),
                                    server: Some(server_name.clone()),
                                    comment: Some(lease.name.as_bytes().into()),
                                    // the rest RouterOS's own: a lease taken over has only
                                    // its client, name, MAC and address in NetBox
                                    address_lists: Default::default(),
                                    allow_dual_stack_queue: false,
                                    always_broadcast: false,
                                    block_access: false,
                                    client_id: None,
                                    dhcp_option: None,
                                    dhcp_option_set: None,
                                    disabled: false,
                                    insert_queue_before: None,
                                    lease_time: Duration::ZERO,
                                    parent_queue: value::HasNone::NoneValue,
                                    queue_type: None,
                                    rate_limit: None,
                                    routes: None,
                                    use_src_mac: false,
                                },
                            );
                        }
                        self.ipv_4_pool.entry(server_name).or_default().0.ranges =
                            dhcp_ranges
                                .iter()
                                .map(|r| pool_range(r).into())
                                .collect();
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
                    comment: Some(firewall_comment(&["close-dns", &if_name.to_string(), &protocol.to_string()])),
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

/// What the generator's firewall entries begin their comment with: only those are the
/// provisioner's, the others on the device stay (the `owned` firewall menus of the model).
const FIREWALL_PREFIX: &str = "netbox-provisioner:";

/// The comment of a firewall entry: the prefix and what the entry is, unique in its menu, as
/// the order of the rules finds them by it (`move [find comment=…]`).
fn firewall_comment(parts: &[&str]) -> AsciiString {
    AsciiString::from(format!("{FIREWALL_PREFIX} {}", parts.join(" ")).as_str())
}
