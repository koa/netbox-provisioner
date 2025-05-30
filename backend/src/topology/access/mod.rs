use crate::topology::{
    CablePort, Device, DeviceId, Interface, InterfaceId, PhysicalPortId, Topology, VlanData,
    VlanId, VxlanData, VxlanId, WlanData, WlanGroupData, WlanGroupId, WlanId,
};
use ipnet::IpNet;
use mikrotik_model::ascii::AsciiString;
use std::{
    collections::{BTreeSet, HashSet},
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

pub mod graphql;

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceAccess {
    topology: Arc<Topology>,
    id: DeviceId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct InterfaceAccess {
    topology: Arc<Topology>,
    id: InterfaceId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct WlanGroupAccess {
    topology: Arc<Topology>,
    id: WlanGroupId,
}

#[derive(Clone, PartialEq, Eq)]
pub struct VxlanAccess {
    topology: Arc<Topology>,
    id: VxlanId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct VlanAccess {
    topology: Arc<Topology>,
    pub id: VlanId,
}
#[derive(Clone, PartialEq, Eq)]
pub struct WlanAccess {
    topology: Arc<Topology>,
    id: WlanId,
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
    pub fn primary_ip_v4(&self) -> Option<Ipv4Addr> {
        self.data().and_then(|d| d.primary_ip_v4)
    }
    pub fn primary_ip_v6(&self) -> Option<Ipv6Addr> {
        self.data().and_then(|d| d.primary_ip_v6)
    }

    pub fn loopback_ip(&self) -> Option<IpAddr> {
        self.data().and_then(|d| d.loopback_ip)
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

    pub fn vlans(&self) -> impl Iterator<Item = VlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.vlans.iter().cloned())
            .map(|id| VlanAccess {
                topology: self.topology.clone(),
                id,
            })
    }

    pub fn vxlan(&self) -> HashSet<VxlanAccess> {
        self.vlans().filter_map(|vl| vl.vxlan()).collect()
    }
}

impl InterfaceAccess {
    pub fn id(&self) -> InterfaceId {
        self.id
    }
    pub fn name(&self) -> &str {
        self.data().map(|d| d.name.as_ref()).unwrap_or_default()
    }
    pub fn label(&self) -> Option<&str> {
        self.data()
            .map(|d| d.label.as_ref())
            .filter(|l| !l.is_empty())
    }

    pub fn use_ospf(&self) -> bool {
        self.data().map(|d| d.use_ospf).unwrap_or(false)
    }
    pub fn external_port(&self) -> Option<PhysicalPortId> {
        self.data().and_then(|d| d.external)
    }
    pub fn is_ethernet_port(&self) -> bool {
        self.external_port()
            .map(|p| p.is_ethernet())
            .unwrap_or(false)
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
    pub fn interface_name(&self) -> Option<AsciiString> {
        self.external_port().map(|port| {
            if let Some(label) = self.label() {
                let mut name = port.short_name().0.to_vec();
                name.push(b'-');
                for char in label.chars() {
                    if char.is_ascii_alphanumeric() {
                        name.push(char as u8);
                    } else {
                        match char {
                            '-' | '.' => name.push(b'-'),
                            'ä' => name.extend_from_slice(b"ae"),
                            'ö' => name.extend_from_slice(b"oe"),
                            'ü' => name.extend_from_slice(b"ue"),
                            _ => {}
                        }
                    }
                }
                name.into_boxed_slice().into()
            } else {
                port.short_name()
            }
        })
    }
    pub fn untagged_vlan(&self) -> Option<VlanAccess> {
        self.data().and_then(|d| d.vlan).map(|id| VlanAccess {
            topology: self.topology.clone(),
            id,
        })
    }
    pub fn tagged_vlans(&self) -> impl Iterator<Item = VlanAccess> {
        self.data().into_iter().flat_map(|data| {
            data.tagged_vlans.iter().copied().map(|id| VlanAccess {
                topology: self.topology.clone(),
                id,
            })
        })
    }
    pub fn bridge(&self) -> Option<InterfaceAccess> {
        self.data()
            .and_then(|d| d.bridge)
            .map(|id| InterfaceAccess {
                topology: self.topology.clone(),
                id,
            })
    }
}
impl Debug for InterfaceAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let device = self.device();
        let device_name = device.as_ref().map(|d| d.name()).unwrap_or_default();
        let if_name = self.name();
        write!(f, "InterfaceAccess({:?};{device_name}:{if_name})", self.id)
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
    pub fn mgmt_vlan(&self) -> Option<VlanAccess> {
        self.data().and_then(|d| d.mgmt_vlan).map(|id| VlanAccess {
            topology: self.topology.clone(),
            id,
        })
    }
    pub fn wlan(&self) -> impl Iterator<Item = WlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.wlans.iter().cloned())
            .map(|id| WlanAccess {
                topology: self.topology.clone(),
                id,
            })
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
    pub fn interface_terminations(&self) -> Box<[InterfaceAccess]> {
        self.data()
            .map(|d| {
                d.interface_terminations
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
    pub fn vlan_terminations(&self) -> Box<[VlanAccess]> {
        self.data()
            .map(|d| {
                d.vlan_terminations
                    .iter()
                    .copied()
                    .map(|id| VlanAccess {
                        topology: self.topology.clone(),
                        id,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn vteps(&self) -> Box<[IpAddr]> {
        Box::from_iter(
            self.interface_terminations()
                .iter()
                .filter_map(InterfaceAccess::device)
                .chain(
                    self.vlan_terminations()
                        .iter()
                        .flat_map(|vl| vl.wlan())
                        .filter_map(|wlan| wlan.wlan_group())
                        .flat_map(|group| group.aps().into_iter().chain(group.controller())),
                )
                .filter_map(|dev| dev.primary_ip_v4())
                .map(IpAddr::V4)
                .collect::<BTreeSet<IpAddr>>(),
        )
    }
}
impl Debug for VxlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VxlanAccess({};{})",
            self.id.0,
            self.name().unwrap_or_default()
        )
    }
}
impl Hash for VxlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl VlanAccess {
    pub fn data(&self) -> Option<&VlanData> {
        self.topology.vlans.get(&self.id)
    }
    pub fn name(&self) -> Option<&str> {
        self.data().map(|v| v.name.as_ref())
    }
    pub fn vlan_id(&self) -> Option<u16> {
        self.data().map(|v| v.vlan_id)
    }
    pub fn vxlan(&self) -> Option<VxlanAccess> {
        self.data().and_then(|v| v.vxlan).map(|id| VxlanAccess {
            topology: self.topology.clone(),
            id,
        })
    }
    pub fn wlan(&self) -> impl Iterator<Item = WlanAccess> {
        self.data()
            .into_iter()
            .flat_map(|d| d.wlans.iter().cloned())
            .map(|id| WlanAccess {
                topology: self.topology.clone(),
                id,
            })
    }
}
impl Debug for VlanAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VlanAccess({};{}({}))",
            self.id.0,
            self.name().unwrap_or_default(),
            self.vlan_id().unwrap_or_default()
        )
    }
}
impl Hash for VlanAccess {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl WlanAccess {
    pub fn data(&self) -> Option<&WlanData> {
        self.topology.wlans.get(&self.id)
    }
    pub fn vlan(&self) -> Option<VlanAccess> {
        self.data().and_then(|d| d.vlan).map(|id| VlanAccess {
            topology: self.topology.clone(),
            id,
        })
    }
    pub fn wlan_group(&self) -> Option<WlanGroupAccess> {
        self.data().map(|d| d.wlan_group).map(|id| WlanGroupAccess {
            topology: self.topology.clone(),
            id,
        })
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
