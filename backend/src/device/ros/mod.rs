//! Scope: The generator's frame: the models of the menus it writes (`mikrotik_model!`, whose
//! types are private to this module and its children), names several steps share and the order
//! of the base config's steps; not a step's own content (its topic module).
use crate::{
    Error,
    device::ros::{
        hw_facts::{RadioPackage, build_ethernet_ports},
        l2::{L2Port, L2Setup, NameGenerator},
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
        CapsManProvisioningHwSupportedModes, InterfaceEthernetByDefaultName, InterfaceEthernetCfg,
        InterfaceListByName, InterfaceListCfg, IpNeighborDiscoverySettingsCfg, IpSshCfg,
        IpSshForwardingEnabled, ToolMacServerCfg, WifiChannelBand,
    },
    value,
};
use std::{
    collections::{BTreeSet, HashMap},
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
mod maintenance;
mod mtu;
mod ospf;
mod poe;
pub mod progress;
pub(crate) mod radio;
mod services;
pub(crate) mod speed;
mod switch_menu;
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

mikrotik_model!(
    name = SwitchMenuData,
    detect = new,
    fields(
        switch(single = "interface/ethernet/switch"),
        switch_vlan(by_key(
            path = "interface/ethernet/switch/vlan",
            key = vlanId
        )),
        egress_vlan_tag(by_id(
            path = "interface/ethernet/switch/egress-vlan-tag",
            keys(vlan_id)
        )),
        ingress_vlan_translation(by_id(
            path = "interface/ethernet/switch/ingress-vlan-translation",
            keys(ports, customer_vid)
        )),
    ),
);

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
        let syslog::Logging {
            actions: logging_action,
            rules: logging,
        } = syslog::routeros_logging();
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
}

/// What the generator's firewall entries begin their comment with: only those are the
/// provisioner's, the others on the device stay (the `owned` firewall menus of the model).
const FIREWALL_PREFIX: &str = "netbox-provisioner:";

/// The comment of a firewall entry: the prefix and what the entry is, unique in its menu, as
/// the order of the rules finds them by it (`move [find comment=…]`).
fn firewall_comment(parts: &[&str]) -> AsciiString {
    AsciiString::from(format!("{FIREWALL_PREFIX} {}", parts.join(" ")).as_str())
}
