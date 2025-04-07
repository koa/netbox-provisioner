use crate::{netbox::NetboxError, topology::fetch::build_topology};
use access::DeviceAccess;
use async_graphql::{Interface, SimpleObject, Union};
use ipnet::IpNet;
use lazy_static::lazy_static;
use log::error;
use mikrotik_model::ascii::AsciiString;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::{Mutex, MutexGuard};

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
        if self.data.lock().await.is_none() {
            if let Err(e) = self.fetch().await {
                error!("Cannot fetch topology data: {e}");
            }
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Topology {
    devices: HashMap<DeviceId, Device>,
    interfaces: HashMap<InterfaceId, Interface>,
    cable_path_endpoints: HashMap<CablePort, HashSet<CablePort>>,
    vxlans: HashMap<VxlanId, VxlanData>,
    wlan_groups: HashMap<WlanGroupId, WlanGroupData>,
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
    wlan_vxlan: Option<VxlanId>,
    //vxlans: Box<[VxlanId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanData {
    name: Box<str>,
    vni: u32,
    terminations: HashSet<InterfaceId>,
    wlan_group: Option<WlanGroupId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanGroupData {
    transport_vxlan: Option<VxlanId>,
    controller: DeviceId,
    aps: Box<[DeviceId]>,
    wlans: Box<[WlanData]>,
}
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct WlanData {
    ssid: Box<str>,
    vlan: u16,
    wlan_auth: WlanAuth,
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
    device: DeviceId,
    external: Option<PhysicalPortId>,
    ips: Box<[IpNet]>,
    use_ospf: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum PhysicalPortId {
    Ethernet(u16),
    SfpSfpPlus(u16),
    Wifi(u16),
    Wlan(u16),
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
        })
    }
}
impl FromStr for PhysicalPortId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(c) = ETHER_PORT_PATTERN
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
pub struct WlanGroupId(pub u32);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
enum CablePort {
    Interface(InterfaceId),
    FrontPort(u32),
    RearPort(u32),
}
