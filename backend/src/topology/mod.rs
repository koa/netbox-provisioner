use crate::{
    netbox::{NetboxError, fetch_topology::ResponseData},
    topology::fetch::build_topology,
};
use access::device::DeviceAccess;
use async_graphql::{Interface, SimpleObject, Union};
use ipnet::IpNet;
use lazy_static::lazy_static;
use log::{error, info};
use mikrotik_model::ascii::AsciiString;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    hash::Hash,
    net::IpAddr,
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, MutexGuard},
    time::Instant,
};

pub mod access;
pub mod fetch;
pub mod findings;
pub mod natural;
mod graphql;
pub mod test;

#[derive(Debug, Default, Clone)]
pub struct TopologyHolder {
    data: Arc<Mutex<Option<Arc<Topology>>>>,
}

impl TopologyHolder {
    pub async fn fetch(&self) -> Result<(), NetboxError> {
        let data_ref = self.data.clone();
        match build_topology().await {
            Ok(value) => {
                data_ref.lock().await.replace(Arc::new(value));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }
    pub async fn topo_lock(&self) -> MutexGuard<'_, Option<Arc<Topology>>> {
        let outdated = {
            let guard = self.data.lock().await;
            if let Some(data) = &*guard {
                let duration = data.fetch_time.elapsed();
                duration > Duration::from_secs(10)
            } else {
                true
            }
        };
        if outdated {
            info!("Fetch Topology");
            if let Err(e) = self.fetch().await {
                error!("Cannot fetch topology data: {e}");
            }
            info!("Topology fetched");
        }
        self.data.lock().await
    }

    /// The topology as fetched last (again if it is old), `None` when NetBox couldn't be read.
    pub async fn current(&self) -> Option<Arc<Topology>> {
        self.topo_lock().await.as_ref().cloned()
    }
    pub async fn devices(&self) -> Box<[DeviceAccess]> {
        if let Some(device) = self.topo_lock().await.as_ref().cloned() {
            device.list_devices().collect()
        } else {
            Box::default()
        }
    }
    pub async fn devices_by_id(&self, id: DeviceId) -> Option<DeviceAccess> {
        if let Some(topo) = self.topo_lock().await.as_ref().cloned() {
            topo.get_device_by_id(&id)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topology {
    fetch_time: Instant,
    devices: HashMap<DeviceId, Device>,
    sites: HashMap<SiteId, SiteData>,
    interfaces: HashMap<InterfaceId, Interface>,
    front_ports: HashMap<FrontPortId, FrontPort>,
    rear_ports: HashMap<RearPortId, RearPort>,
    circuit_terminations: HashMap<CircuitTerminationId, CircuitTermination>,
    cables: HashMap<CableId, Cable>,
    vxlans: HashMap<VxlanId, VxlanData>,
    wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
    wlans: HashMap<WlanId, WlanData>,
    vlan_groups: HashMap<VlanGroupId, VlanGroupData>,
    vlans: HashMap<VlanId, VlanData>,
    ip_addresses: HashMap<IpAddressId, IpAddressData>,
    ip_prefixes: HashMap<IpPrefixId, IpPrefixData>,
    ip_ranges: HashMap<IpRangeId, IpRangeData>,
    ip_range_idx: HashMap<IpNet, Box<[IpRangeId]>>,
    /// `findings::collect`, computed when the topology is built (`Topology::with_findings`)
    findings: Box<[findings::Finding]>,
    /// the NetBox response it is built from, which a helper's plan is tried on
    /// (`helper::preview`); none in tests built from parts
    source: Option<Source>,
}

/// The NetBox response a topology is built from: compared by identity and not printed, it is
/// the whole of NetBox.
#[derive(Clone)]
pub struct Source(pub Arc<ResponseData>);
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Source {}
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Source")
    }
}

impl Topology {
    /// The prefixes of `pool` for IPv6 or IPv4, in the order they are filled: ascending
    /// (`docs/concepts/config-helper.md`, Pools).
    pub(crate) fn pool_prefixes(&self, pool: PrefixPool, v6: bool) -> Vec<(IpPrefixId, IpNet)> {
        let mut prefixes = self
            .ip_prefixes
            .iter()
            .filter(|(_, prefix)| {
                prefix.pool == Some(pool) && matches!(prefix.prefix, IpNet::V6(_)) == v6
            })
            .map(|(id, prefix)| (*id, prefix.prefix))
            .collect::<Vec<_>>();
        prefixes.sort_by_key(|(_, net)| (net.network(), net.prefix_len()));
        prefixes
    }
    /// The IPv4 prefixes scoped to `site` that can hold networks: no pool's, none that is a
    /// network itself (with an address of its own length), by address.
    pub(crate) fn customer_parents(&self, site: SiteId) -> Vec<(IpPrefixId, IpNet)> {
        let mut prefixes = self
            .ip_prefixes
            .iter()
            .filter(|(_, prefix)| {
                prefix.site == Some(site)
                    && prefix.pool.is_none()
                    && prefix.addresses.is_empty()
                    && matches!(prefix.prefix, IpNet::V4(_))
            })
            .map(|(id, prefix)| (*id, prefix.prefix))
            .collect::<Vec<_>>();
        prefixes.sort_by_key(|(_, net)| (net.network(), net.prefix_len()));
        prefixes
    }
    pub(crate) fn prefix_data(&self, id: IpPrefixId) -> Option<&IpPrefixData> {
        self.ip_prefixes.get(&id)
    }
    /// This topology built again from its NetBox response after `change` (the JSON of
    /// `ResponseData`): what NetBox will be after a helper's plan. `None` for a topology not
    /// built from a response (tests built from parts).
    pub(crate) fn rebuilt_with(
        &self,
        change: impl FnOnce(&mut serde_json::Value),
    ) -> Option<Result<Topology, serde_json::Error>> {
        let source = self.source.as_ref()?;
        Some((|| {
            let mut json = serde_json::to_value(&*source.0)?;
            change(&mut json);
            let data: ResponseData = serde_json::from_value(json)?;
            Ok(fetch::build_topology_from(data, Instant::now()))
        })())
    }

    /// Whether `device` is an AP of a Wireless LAN Group whose radios NetBox has as `wlanN`
    /// (`/interface/wireless`): such a radio can only be a CAP of the legacy CAPsMAN
    /// (`/caps-man`), which forwards the clients' traffic to the controller, so the AP needs
    /// neither the WLANs' VLANs nor their VXLAN. A CAP with `wifiN` radios, or without
    /// radios in NetBox, is one of the `/interface/wifi` CAPsMAN.
    pub fn is_legacy_cap(&self, device: DeviceId) -> bool {
        self.devices
            .get(&device)
            .is_some_and(|data| data.wlan_ap_of.is_some())
            && self.interfaces.values().any(|interface| {
                interface.device == device
                    && matches!(interface.external, Some(PhysicalPortId::Wlan(_)))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    pub name: Box<str>,
    pub site: Option<SiteId>,
    pub ports: HashSet<CablePort>,
    pub primary_ip: Option<IpAddressId>,
    pub primary_ip_v4: Option<IpAddressId>,
    pub primary_ip_v6: Option<IpAddressId>,
    pub loopback_ip: Option<IpAddressId>,
    pub credentials: Option<Box<str>>,
    pub has_routeros: bool,
    pub serial: Option<Box<str>>,
    pub wlan_controller_of: Option<WlanGroupId>,
    pub wlan_ap_of: Option<WlanGroupId>,
    pub role: Option<DeviceRole>,
    /// Slug of the topmost region of the device's site: the country's ISO 3166 code.
    pub country: Option<Box<str>>,
    /// From the config context (`dns_servers`): the DNS servers of its client networks.
    pub dns_servers: Box<[IpAddr]>,
    /// Custom field `propagate_default_route`: its OSPF announces its default route.
    pub propagate_default_route: bool,
}

/// Device-role slugs relevant to config generation. Other NetBox roles (anything not
/// listed here) are intentionally not modeled - `wlan-ap`/`wlan-controller` behavior
/// is already derived from the `wlan_group`/`controller` custom fields, not from this
/// enum (see `Device.wlan_ap_of`/`wlan_controller_of`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum DeviceRole {
    Router,
    Switch,
    WlanBridge,
}

impl DeviceRole {
    pub fn from_slug(slug: &str) -> Option<DeviceRole> {
        match slug {
            "router" => Some(DeviceRole::Router),
            "switch" => Some(DeviceRole::Switch),
            "wlan-bridge" => Some(DeviceRole::WlanBridge),
            _ => None,
        }
    }
}

/// A NetBox site: the UI chooses one before its devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteData {
    pub name: Box<str>,
    pub devices: Box<[DeviceId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// The VXLAN of a VLAN group with a `vxlan_vni`: a VLAN trunk carrying, tagged,
/// every VLAN of the group that is used on more than one device (see
/// `VlanAccess::tunnel`). Its id is the group's id.
pub struct VxlanData {
    pub name: Box<str>,
    pub vni: u32,
    pub vlans: Box<[VlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanData {
    pub name: Box<str>,
    pub vlan_id: u16,
    pub group: VlanGroupId,
    pub terminations: Box<[InterfaceId]>,
    pub vxlan: Option<VxlanId>,
    pub wlans: Box<[WlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanGroupData {
    pub name: Box<str>,
    pub mgmt_vlan: Option<VlanId>,
    pub controller: DeviceId,
    pub aps: Box<[DeviceId]>,
    pub wlans: Box<[WlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanGroupData {
    pub vlans: Box<[VlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanData {
    pub ssid: Box<str>,
    pub vlan: Option<VlanId>,
    pub wlan_auth: WlanAuth,
    pub wlan_group: WlanGroupId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpRangeData {
    pub is_dhcp: bool,
    pub net: IpNet,
    pub start: IpAddr,
    pub end: IpAddr,
    pub prefix: Option<IpPrefixId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpAddressData {
    pub ip: IpNet,
    pub interface: Option<InterfaceId>,
    pub prefix: Option<IpPrefixId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpPrefixData {
    pub prefix: IpNet,
    pub addresses: Box<[IpAddressId]>,
    pub children: Box<[IpPrefixId]>,
    pub parent: Option<IpPrefixId>,
    pub ranges: Box<[IpRangeId]>,
    /// NetBox tag `loopback` or `transfer`: the prefix is part of that pool.
    pub pool: Option<PrefixPool>,
    /// its scope, if that is a site: a customer network's parent
    pub site: Option<SiteId>,
}
/// What a prefix with one of the tags `loopback` or `transfer` hands out: all prefixes with
/// the same tag and IP version form one pool (`docs/concepts/config-helper.md`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, async_graphql::Enum)]
pub enum PrefixPool {
    /// `/32` and `/128` on `lo`
    Loopback,
    /// The networks between two routers
    Transfer,
}
#[derive(Debug, Clone, PartialEq, Eq, Union)]
pub enum WlanAuth {
    Wpa(WlanWpaSettings),
    Open(WlanOpenSettings),
}
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanWpaSettings {
    key: Box<str>,
}
impl WlanWpaSettings {
    pub fn new(key: impl Into<Box<str>>) -> Self {
        WlanWpaSettings { key: key.into() }
    }
    pub fn psk(&self) -> &str {
        &self.key
    }
}
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanOpenSettings {
    pub use_owe: bool,
}
impl Device {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn credentials(&self) -> Option<&str> {
        self.credentials.as_ref().map(|s| s.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct Interface {
    pub name: Box<str>,
    pub label: Box<str>,
    pub device: DeviceId,
    pub external: Option<PhysicalPortId>,
    pub port_type: Option<PortType>,
    pub vlan: Option<VlanId>,
    pub tagged_vlans: Box<[VlanId]>,
    pub ips: Box<[IpAddressId]>,
    pub use_ospf: bool,
    pub enable_dhcp_client: bool,
    pub enable_dhcp_server: bool,
    /// NetBox tag `nat`: the uplink whose network doesn't route back to the device's ones;
    /// what leaves through it is masqueraded
    pub nat: bool,
    pub bridge: Option<InterfaceId>,
    pub cable: Option<CableId>,
    /// NetBox `poe_mode`: `Pse` feeds the device at the other end of its cable
    pub poe: Option<PoeMode>,
    /// NetBox `mtu`: the L3 MTU of a physical port, for both ends of its cable
    pub mtu: Option<u16>,
    /// Set on a radio port with an RF role in NetBox (a point-to-point bridge end).
    pub radio: Option<Radio>,
    /// The NetBox wireless link it is an end of, with or without RF role.
    pub wireless_link: Option<WirelessLinkEnd>,
    /// NetBox type 802.11ad or ay: a 60 GHz radio, `wlan60-<N>` on RouterOS.
    pub sixty_ghz: bool,
}

/// An interface's end of a NetBox wireless link.
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct WirelessLinkEnd {
    /// NetBox id of the wireless link
    pub id: u32,
    /// the interface at the other end
    pub peer: Option<InterfaceId>,
    pub link: RadioLink,
}

/// NetBox `poe_mode` of an interface.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, async_graphql::Enum)]
pub enum PoeMode {
    /// powering equipment: it feeds what is cabled to it
    Pse,
    /// powered device: fed through it
    Pd,
}

impl PoeMode {
    /// as NetBox' API takes it
    pub fn value(self) -> &'static str {
        match self {
            PoeMode::Pse => "pse",
            PoeMode::Pd => "pd",
        }
    }
}

/// A radio port's settings from NetBox: its RF role, channel and wireless link.
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct Radio {
    pub role: RadioRole,
    /// Center frequency in MHz; `None` lets the radio pick (`frequency=auto`).
    pub frequency: Option<u32>,
    /// Channel width in MHz.
    pub channel_width: Option<u32>,
    /// Fixed transmit power in dBm; `None` leaves it to the radio's defaults.
    pub tx_power: Option<i8>,
    pub link: Option<RadioLink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum RadioRole {
    /// NetBox RF role `ap`: the side the station connects to (`mode=bridge`).
    Ap,
    /// NetBox RF role `station` (`mode=station-bridge`).
    Station,
}

/// The NetBox wireless link a radio port is an end of.
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct RadioLink {
    pub ssid: Box<str>,
    /// Whether the link has an authentication type other than open.
    pub encrypted: bool,
    /// Pre-shared key; `None` for an open link or when NetBox returns none (e.g. an
    /// export without secrets).
    pub psk: Option<Box<str>>,
    /// RouterOS `country` (5 GHz) or `region` (60 GHz) of both ends, overriding the
    /// one derived from the site's region (`rf_country`).
    pub country: Option<Box<str>>,
    /// RouterOS `frequency-mode` of both ends (`rf_frequency_mode`).
    pub frequency_mode: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Copy)]
pub enum PortType {
    Ethernet,
    Wireless,
    Loopback,
    Bridge,
}

/// A physical port, identified by its RouterOS default name (the NetBox interface
/// name must match it exactly, see `docs/netbox-modeling.md`). The numbering and
/// short names mirror `mikrotik_model::hwconfig::EthernetNamePattern`, which builds
/// the per-board port list these are looked up in.
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum PhysicalPortId {
    Ethernet(u16),
    SfpSfpPlus(u16),
    /// `sfp28-{id}`: a 25G SFP28 cage
    Sfp28(u16),
    Sfp(u16),
    Combo(u16),
    /// `qsfpplus{port}-{lane}`: RouterOS always names QSFP+ ports per lane, even
    /// without a breakout cable (`qsfpplus1-1` is then the whole 40G port).
    QsfpPlus(u16, u16),
    Wifi(u16),
    Wlan(u16),
    /// `wlan60-{id}`: a 60 GHz radio (`/interface/w60g`)
    W60g(u16),
    Loopback,
}
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct FrontPort {
    pub name: Box<str>,
    pub device: DeviceId,
    pub rear_port: Option<RearPortId>,
    pub cable: Option<CableId>,
}
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct RearPort {
    pub name: Box<str>,
    pub device: DeviceId,
    pub front_port: Option<FrontPortId>,
    pub cable: Option<CableId>,
}
/// An end of a circuit (a leased line, a fiber between buildings): a cable path goes on at the
/// circuit's other end, `peer`, as NetBox' trace does.
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct CircuitTermination {
    /// the circuit's `cid`
    pub circuit: Box<str>,
    /// none where the circuit has another number of ends than two (one to a provider network)
    pub peer: Option<CircuitTerminationId>,
    pub cable: Option<CableId>,
}
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct Cable {
    pub port_a: Box<[CablePort]>,
    pub port_b: Box<[CablePort]>,
}
lazy_static! {
    static ref ETHER_PORT_PATTERN: Regex = Regex::new("^ether([0-9]+)$").unwrap();
    static ref SFP_PLUS_PORT_PATTERN: Regex = Regex::new("^sfp-sfpplus([0-9]+)$").unwrap();
    static ref SFP28_PORT_PATTERN: Regex = Regex::new("^sfp28-([0-9]+)$").unwrap();
    static ref SFP_PORT_PATTERN: Regex = Regex::new("^sfp([0-9]+)$").unwrap();
    static ref COMBO_PORT_PATTERN: Regex = Regex::new("^combo([0-9]+)$").unwrap();
    static ref QSFP_PLUS_PORT_PATTERN: Regex = Regex::new("^qsfpplus([0-9]+)-([0-9]+)$").unwrap();
    static ref WIFI_PORT_PATTERN: Regex = Regex::new("^wifi([0-9]+)$").unwrap();
    static ref WLAN_PORT_PATTERN: Regex = Regex::new("^wlan([0-9]+)$").unwrap();
    static ref W60G_PORT_PATTERN: Regex = Regex::new("^wlan60-([0-9]+)$").unwrap();
}
impl PhysicalPortId {
    pub fn short_name(&self) -> AsciiString {
        AsciiString::from(match self {
            PhysicalPortId::Ethernet(id) => format!("e{id:02}"),
            PhysicalPortId::SfpSfpPlus(id) | PhysicalPortId::Sfp(id) => format!("s{id:02}"),
            PhysicalPortId::Sfp28(id) => format!("x{id:02}"),
            PhysicalPortId::Combo(id) => format!("c{id:02}"),
            PhysicalPortId::QsfpPlus(port, lane) => format!("q{port:02}-{lane}"),
            PhysicalPortId::Wifi(id) | PhysicalPortId::Wlan(id) | PhysicalPortId::W60g(id) => {
                format!("w{id:02}")
            }
            PhysicalPortId::Loopback => "lo".to_string(),
        })
    }
    pub fn default_name(&self) -> Option<AsciiString> {
        match self {
            PhysicalPortId::Ethernet(_)
            | PhysicalPortId::SfpSfpPlus(_)
            | PhysicalPortId::Sfp28(_)
            | PhysicalPortId::Sfp(_)
            | PhysicalPortId::Combo(_)
            | PhysicalPortId::QsfpPlus(_, _) => Some(self.to_string()),
            PhysicalPortId::Wifi(_)
            | PhysicalPortId::Wlan(_)
            | PhysicalPortId::W60g(_)
            | PhysicalPortId::Loopback => None,
        }
        .map(AsciiString::from)
    }
    pub fn is_ethernet(&self) -> bool {
        match self {
            PhysicalPortId::Ethernet(_)
            | PhysicalPortId::SfpSfpPlus(_)
            | PhysicalPortId::Sfp28(_)
            | PhysicalPortId::Sfp(_)
            | PhysicalPortId::Combo(_)
            | PhysicalPortId::QsfpPlus(_, _) => true,
            PhysicalPortId::Wifi(_)
            | PhysicalPortId::Wlan(_)
            | PhysicalPortId::W60g(_)
            | PhysicalPortId::Loopback => false,
        }
    }
}
impl FromStr for PhysicalPortId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let number = |pattern: &Regex, group: usize| {
            pattern
                .captures(s)
                .and_then(|c| c.get(group).and_then(|c| u16::from_str(c.as_str()).ok()))
        };
        if s == "lo" {
            Ok(PhysicalPortId::Loopback)
        } else if let Some(id) = number(&ETHER_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Ethernet(id))
        } else if let Some(id) = number(&SFP_PLUS_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::SfpSfpPlus(id))
        } else if let Some(id) = number(&SFP28_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Sfp28(id))
        } else if let Some(id) = number(&SFP_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Sfp(id))
        } else if let Some(id) = number(&COMBO_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Combo(id))
        } else if let (Some(port), Some(lane)) = (
            number(&QSFP_PLUS_PORT_PATTERN, 1),
            number(&QSFP_PLUS_PORT_PATTERN, 2),
        ) {
            Ok(PhysicalPortId::QsfpPlus(port, lane))
        } else if let Some(id) = number(&WIFI_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Wifi(id))
        } else if let Some(id) = number(&WLAN_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::Wlan(id))
        } else if let Some(id) = number(&W60G_PORT_PATTERN, 1) {
            Ok(PhysicalPortId::W60g(id))
        } else {
            Err(())
        }
    }
}

impl Display for PhysicalPortId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PhysicalPortId::Ethernet(id) => write!(f, "ether{id}"),
            PhysicalPortId::SfpSfpPlus(id) => write!(f, "sfp-sfpplus{id}"),
            PhysicalPortId::Sfp28(id) => write!(f, "sfp28-{id}"),
            PhysicalPortId::Sfp(id) => write!(f, "sfp{id}"),
            PhysicalPortId::Combo(id) => write!(f, "combo{id}"),
            PhysicalPortId::QsfpPlus(port, lane) => write!(f, "qsfpplus{port}-{lane}"),
            PhysicalPortId::Wifi(id) => write!(f, "wifi{id}"),
            PhysicalPortId::Wlan(id) => write!(f, "wlan{id}"),
            PhysicalPortId::W60g(id) => write!(f, "wlan60-{id}"),
            PhysicalPortId::Loopback => f.write_str("lo"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct InterfaceId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct DeviceId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct SiteId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct VxlanId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct VlanId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct WlanId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct VlanGroupId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct WlanGroupId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct FrontPortId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct RearPortId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct CircuitTerminationId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct CableId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct IpRangeId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct IpAddressId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct IpPrefixId(pub u32);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum CablePort {
    Interface(InterfaceId),
    FrontPort(FrontPortId),
    RearPort(RearPortId),
    CircuitTermination(CircuitTerminationId),
}

impl From<u32> for DeviceId {
    fn from(value: u32) -> Self {
        DeviceId(value)
    }
}
impl From<u32> for SiteId {
    fn from(value: u32) -> Self {
        SiteId(value)
    }
}
impl From<u32> for VxlanId {
    fn from(value: u32) -> Self {
        VxlanId(value)
    }
}
impl From<u32> for VlanId {
    fn from(value: u32) -> Self {
        VlanId(value)
    }
}
impl From<u32> for WlanId {
    fn from(value: u32) -> Self {
        WlanId(value)
    }
}
impl From<u32> for VlanGroupId {
    fn from(value: u32) -> Self {
        VlanGroupId(value)
    }
}
impl From<u32> for WlanGroupId {
    fn from(value: u32) -> Self {
        WlanGroupId(value)
    }
}
impl From<u32> for FrontPortId {
    fn from(value: u32) -> Self {
        FrontPortId(value)
    }
}
impl From<u32> for RearPortId {
    fn from(value: u32) -> Self {
        RearPortId(value)
    }
}
impl From<u32> for CircuitTerminationId {
    fn from(value: u32) -> Self {
        CircuitTerminationId(value)
    }
}
impl From<u32> for CableId {
    fn from(value: u32) -> Self {
        CableId(value)
    }
}
impl From<u32> for IpRangeId {
    fn from(value: u32) -> Self {
        IpRangeId(value)
    }
}
impl From<u32> for IpAddressId {
    fn from(value: u32) -> Self {
        IpAddressId(value)
    }
}
impl Display for IpAddressId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl From<u32> for IpPrefixId {
    fn from(value: u32) -> Self {
        IpPrefixId(value)
    }
}
impl Display for IpPrefixId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u32> for InterfaceId {
    fn from(value: u32) -> Self {
        InterfaceId(value)
    }
}
