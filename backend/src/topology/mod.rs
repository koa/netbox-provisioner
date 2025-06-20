use crate::{netbox::NetboxError, topology::fetch::build_topology};
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
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
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
mod graphql;
#[cfg(test)]
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
    pub async fn topo_lock(&self) -> MutexGuard<Option<Arc<Topology>>> {
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
    interfaces: HashMap<InterfaceId, Interface>,
    front_ports: HashMap<FrontPortId, FrontPort>,
    rear_ports: HashMap<RearPortId, RearPort>,
    cables: HashMap<CableId, Cable>,
    //cable_path_endpoints: HashMap<CablePort, HashSet<CablePort>>,
    vxlans: HashMap<VxlanId, VxlanData>,
    wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
    wlans: HashMap<WlanId, WlanData>,
    vlan_groups: HashMap<VlanGroupId, VlanGroupData>,
    vlans: HashMap<VlanId, VlanData>,
    ip_addresses: HashMap<IpAddressId, IpAddressData>,
    ip_prefixes: HashMap<IpPrefixId, IpPrefixData>,
    ip_ranges: HashMap<IpRangeId, IpRangeData>,
    ip_range_idx: HashMap<IpNet, Box<[IpRangeId]>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    pub name: Box<str>,
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
    pub vlans: Box<[VlanId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanData {
    pub name: Box<str>,
    pub vni: u32,
    pub interface_terminations: Box<[InterfaceId]>,
    pub vlan_terminations: Box<[VlanId]>,
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
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanOpenSettings {
    use_owe: bool,
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
    pub bridge: Option<InterfaceId>,
    pub cable: Option<CableId>,
    pub enable_poe: bool,
}

#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Copy)]
pub enum PortType {
    Ethernet,
    Wireless,
    Loopback,
    Bridge,
}

#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum PhysicalPortId {
    Ethernet(u16),
    SfpSfpPlus(u16),
    Wifi(u16),
    Wlan(u16),
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
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct Cable {
    pub port_a: Box<[CablePort]>,
    pub port_b: Box<[CablePort]>,
}
lazy_static! {
    static ref ETHER_PORT_PATTERN: Regex = Regex::new("ether([0-9]+)").unwrap();
    static ref SFP_PLUS_PORT_PATTERN: Regex = Regex::new("sfp-sfpplus([0-9]+)").unwrap();
    static ref WIFI_PORT_PATTERN: Regex = Regex::new("wifi([0-9]+)").unwrap();
    static ref WLAN_PORT_PATTERN: Regex = Regex::new("wlan([0-9]+)").unwrap();
}
impl PhysicalPortId {
    pub fn short_name(&self) -> AsciiString {
        AsciiString::from(match self {
            PhysicalPortId::Ethernet(id) => {
                format!("e{id:02}")
            }
            PhysicalPortId::SfpSfpPlus(id) => {
                format!("s{id:02}")
            }
            PhysicalPortId::Wifi(id) => {
                format!("w{id:02}")
            }
            PhysicalPortId::Wlan(id) => {
                format!("w{id:02}")
            }
            PhysicalPortId::Loopback => "lo".to_string(),
        })
    }
    pub fn default_name(&self) -> Option<AsciiString> {
        match self {
            PhysicalPortId::Ethernet(id) => Some(format!("ether{id}")),
            PhysicalPortId::SfpSfpPlus(id) => Some(format!("sfp-sfpplus{id}")),
            PhysicalPortId::Wifi(_) => None,
            PhysicalPortId::Wlan(_) => None,
            PhysicalPortId::Loopback => None,
        }
        .map(AsciiString::from)
    }
    pub fn is_ethernet(&self) -> bool {
        match self {
            PhysicalPortId::Ethernet(_) => true,
            PhysicalPortId::SfpSfpPlus(_) => true,
            PhysicalPortId::Wifi(_) => false,
            PhysicalPortId::Wlan(_) => false,
            PhysicalPortId::Loopback => false,
        }
    }
}
impl FromStr for PhysicalPortId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "lo" {
            Ok(PhysicalPortId::Loopback)
        } else if let Some(c) = ETHER_PORT_PATTERN
            .captures(s)
            .and_then(|c| c.get(1).and_then(|c| u16::from_str(c.as_str()).ok()))
        {
            Ok(PhysicalPortId::Ethernet(c))
        } else if let Some(c) = SFP_PLUS_PORT_PATTERN
            .captures(s)
            .and_then(|c| c.get(1).and_then(|c| u16::from_str(c.as_str()).ok()))
        {
            Ok(PhysicalPortId::SfpSfpPlus(c))
        } else if let Some(c) = WIFI_PORT_PATTERN
            .captures(s)
            .and_then(|c| c.get(1).and_then(|c| u16::from_str(c.as_str()).ok()))
        {
            Ok(PhysicalPortId::Wifi(c))
        } else if let Some(c) = WLAN_PORT_PATTERN
            .captures(s)
            .and_then(|c| c.get(1).and_then(|c| u16::from_str(c.as_str()).ok()))
        {
            Ok(PhysicalPortId::Wlan(c))
        } else {
            Err(())
        }
    }
}

impl Display for PhysicalPortId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PhysicalPortId::Ethernet(id) => {
                write!(f, "ether{}", id)
            }
            PhysicalPortId::SfpSfpPlus(id) => {
                write!(f, "sfp-sfpplus{}", id)
            }
            PhysicalPortId::Wifi(id) => {
                write!(f, "wifi{}", id)
            }
            PhysicalPortId::Wlan(id) => {
                write!(f, "wlan{}", id)
            }
            PhysicalPortId::Loopback => f.write_str("lo"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct InterfaceId(pub u32);
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Default)]
pub struct DeviceId(pub u32);
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
}

impl From<u32> for DeviceId {
    fn from(value: u32) -> Self {
        DeviceId(value)
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
impl From<u32> for IpPrefixId {
    fn from(value: u32) -> Self {
        IpPrefixId(value)
    }
}
impl From<u32> for InterfaceId {
    fn from(value: u32) -> Self {
        InterfaceId(value)
    }
}
