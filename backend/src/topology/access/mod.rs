use crate::topology::{
    CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, Topology, VxlanData,
    VxlanId, WlanData, WlanGroupData, WlanGroupId,
};
use ipnet::IpNet;
use std::{net::IpAddr, sync::Arc};

pub mod graphql;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAccess {
    topology: Arc<Topology>,
    id: DeviceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAccess {
    topology: Arc<Topology>,
    id: InterfaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanGroupAccess {
    topology: Arc<Topology>,
    id: WlanGroupId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanAccess {
    topology: Arc<Topology>,
    id: VxlanId,
}

impl DeviceAccess {
    pub fn id(&self) -> DeviceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name()).unwrap_or_default()
    }
    pub fn serial(&self) -> Option<&str> {
        self.data().and_then(|d| d.serial.as_deref())
    }
    pub fn primary_ip(&self) -> Option<IpAddr> {
        self.data().and_then(Device::primary_ip)
    }
    pub fn loopback_ip(&self) -> Option<IpAddr> {
        self.data().and_then(|d| d.loopback_ip.clone())
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
    pub fn wlan_controller_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_controller_of)
            .map(|id| WlanGroupAccess {
                topology: self.topology.clone(),
                id,
            })
    }
    pub fn wlan_ap_of(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_ap_of)
            .map(|id| WlanGroupAccess {
                topology: self.topology.clone(),
                id,
            })
    }

    pub fn wlan_vxlan(&self) -> Option<VxlanAccess> {
        self.data()
            .and_then(|d| d.wlan_vxlan)
            .map(|id| VxlanAccess {
                topology: self.topology.clone(),
                id,
            })
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
    pub fn ips(&self) -> &[IpNet] {
        self.data().map(|d| d.ips.as_ref()).unwrap_or_default()
    }
}

impl WlanGroupAccess {
    pub fn data(&self) -> Option<&WlanGroupData> {
        self.topology.wlan_groups.get(&self.id)
    }

    pub fn controller(&self) -> Option<DeviceAccess> {
        self.data().map(|d| DeviceAccess {
            topology: self.topology.clone(),
            id: d.controller,
        })
    }
    pub fn aps(&self) -> Box<[DeviceAccess]> {
        self.data()
            .iter()
            .flat_map(|data| data.aps.iter().copied())
            .map(|id| DeviceAccess {
                topology: self.topology.clone(),
                id,
            })
            .collect()
    }
    pub fn transport_vxlan(&self) -> Option<VxlanAccess> {
        self.data()
            .and_then(|d| d.transport_vxlan)
            .map(|id| VxlanAccess {
                topology: self.topology.clone(),
                id,
            })
    }
    pub fn wlan(&self) -> &[WlanData] {
        self.data().map(|d| d.wlans.as_ref()).unwrap_or_default()
    }
}
impl VxlanAccess {
    pub fn data(&self) -> Option<&VxlanData> {
        self.topology.vxlans.get(&self.id)
    }
    pub fn name(&self) -> Option<&str> {
        self.data().map(|d| d.name.as_ref())
    }
    pub fn vni(&self) -> Option<u32> {
        self.data().map(|d| d.vni)
    }
    pub fn terminations(&self) -> Box<[InterfaceAccess]> {
        self.data()
            .map(|d| {
                d.terminations
                    .iter()
                    .copied()
                    .map(|id| InterfaceAccess {
                        topology: self.topology.clone(),
                        id,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn wlan_group(&self) -> Option<WlanGroupAccess> {
        self.data()
            .and_then(|d| d.wlan_group)
            .map(|id| WlanGroupAccess {
                topology: self.topology.clone(),
                id,
            })
    }
    pub fn vteps(&self) -> Box<[IpAddr]> {
        self.wlan_group()
            .iter()
            .flat_map(|wlan| {
                wlan.aps()
                    .into_iter()
                    .chain(wlan.controller())
                    .filter_map(|device| device.primary_ip())
            })
            .collect()
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
