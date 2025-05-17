use crate::{netbox::NetboxError, topology::fetch::build_topology};
use access::DeviceAccess;
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
    pub async fn devices_by_id(&self, id: u32) -> Option<DeviceAccess> {
        if let Some(topo) = self.topo_lock().await.as_ref().cloned() {
            topo.get_device_by_id(&DeviceId(id))
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
    cable_path_endpoints: HashMap<CablePort, HashSet<CablePort>>,
    vxlans: HashMap<VxlanId, VxlanData>,
    wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
    wlans: HashMap<WlanId, WlanData>,
    vlan_groups: HashMap<VlanGroupId, VlanGroupData>,
    vlans: HashMap<VlanId, VlanData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    name: Box<str>,
    ports: HashSet<CablePort>,
    primary_ip: Option<IpAddr>,
    primary_ip_v4: Option<Ipv4Addr>,
    primary_ip_v6: Option<Ipv6Addr>,
    loopback_ip: Option<IpAddr>,
    credentials: Option<Box<str>>,
    has_routeros: bool,
    serial: Option<Box<str>>,
    wlan_controller_of: Option<WlanGroupId>,
    wlan_ap_of: Option<WlanGroupId>,
    vlans: Box<[VlanId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanData {
    name: Box<str>,
    vni: u32,
    interface_terminations: Box<[InterfaceId]>,
    vlan_terminations: Box<[VlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanData {
    name: Box<str>,
    vlan_id: u16,
    group: VlanGroupId,
    terminations: Box<[InterfaceId]>,
    vxlan: Option<VxlanId>,
    wlans: Box<[WlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanGroupData {
    mgmt_vlan: Option<VlanId>,
    controller: DeviceId,
    aps: Box<[DeviceId]>,
    wlans: Box<[WlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlanGroupData {
    vlans: Box<[VlanId]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanData {
    ssid: Box<str>,
    vlan: Option<VlanId>,
    wlan_auth: WlanAuth,
    wlan_group: WlanGroupId,
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

    pub fn primary_ip(&self) -> Option<IpAddr> {
        self.primary_ip
    }

    pub fn credentials(&self) -> Option<&str> {
        self.credentials.as_ref().map(|s| s.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct Interface {
    name: Box<str>,
    label: Box<str>,
    device: DeviceId,
    external: Option<PhysicalPortId>,
    port_type: Option<PortType>,
    vlan: Option<VlanId>,
    ips: Box<[IpNet]>,
    use_ospf: bool,
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

#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
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

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
enum CablePort {
    Interface(InterfaceId),
    FrontPort(u32),
    RearPort(u32),
}
