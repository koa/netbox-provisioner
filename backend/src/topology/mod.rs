use crate::{netbox::NetboxError, topology::fetch::build_topology};
use ipnet::IpNet;
use lazy_static::lazy_static;
use log::error;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    net::IpAddr,
    str::FromStr,
    sync::Arc,
};
use tokio::sync::{Mutex, MutexGuard};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    name: Box<str>,
    ports: HashSet<CablePort>,
    primary_ip: Option<IpAddr>,
    credentials: Option<Box<str>>,
    has_routeros: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAccess {
    topology: Arc<Topology>,
    id: DeviceId,
}
pub struct InterfaceAccess {
    topology: Arc<Topology>,
    id: InterfaceId,
}

impl DeviceAccess {
    pub fn id(&self) -> DeviceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name()).unwrap_or_default()
    }
    pub fn primary_ip(&self) -> Option<IpAddr> {
        self.data().and_then(|d| Device::primary_ip(d))
    }
    pub fn data(&self) -> Option<&Device> {
        self.topology.devices.get(&self.id)
    }
    pub fn has_routeros(&self) -> bool {
        self.data().map(|d| d.has_routeros).unwrap_or(false)
    }
    pub fn credentials(&self) -> Option<&str> {
        self.data().and_then(|d| Device::credentials(d))
    }
    pub fn interfaces<'a>(&'a self) -> Box<[InterfaceAccess]> {
        self.topology
            .devices
            .get(&self.id)
            .map(|data| {
                data.ports
                    .iter()
                    .filter_map(|p| {
                        if let CablePort::Interface(p) = p {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .copied()
                    .map(|p| InterfaceAccess {
                        topology: self.topology.clone().clone(),
                        id: p,
                    })
                    .collect::<Box<[_]>>()
            })
            .unwrap_or_default()
    }
}

impl InterfaceAccess {
    pub fn id(&self) -> InterfaceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name.as_ref()).unwrap_or_default()
    }
    pub fn external_port(&self) -> Option<PhysicalPortId> {
        self.data().and_then(|d| d.external)
    }

    pub fn data(&self) -> Option<&Interface> {
        self.topology.interfaces.get(&self.id)
    }
    pub fn device(&self) -> Option<DeviceAccess> {
        self.data().map(|data| DeviceAccess {
            topology: self.topology.clone(),
            id: data.device,
        })
    }
    pub fn connected_interfaces(&self) -> Box<[InterfaceAccess]> {
        self.topology
            .cable_path_endpoints
            .get(&CablePort::Interface(self.id))
            .map(|ids| {
                ids.iter()
                    .filter_map(|p| {
                        if let CablePort::Interface(p) = p {
                            Some(InterfaceAccess {
                                topology: self.topology.clone(),
                                id: *p,
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Topology {
    pub fn list_devices<'a>(self: &'a Arc<Self>) -> impl Iterator<Item = DeviceAccess> + use<'a> {
        let topo = self.clone();
        self.devices.keys().map(move |id| DeviceAccess {
            topology: topo.clone(),
            id: *id,
        })
    }
    pub fn get_device_by_id(self: &Arc<Self>, id: &DeviceId) -> Option<DeviceAccess> {
        if self.devices.contains_key(id) {
            Some(DeviceAccess {
                topology: self.clone(),
                id: id.clone(),
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
enum CablePort {
    Interface(InterfaceId),
    FrontPort(u32),
    RearPort(u32),
}
