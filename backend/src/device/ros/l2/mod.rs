use crate::topology::{
    InterfaceId, VlanId,
    access::{DeviceAccess, InterfaceAccess, VlanAccess, VxlanAccess},
};
use ipnet::IpNet;
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
mod test;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Port {
    pub entry: L2PortEntry,
    pub tagged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L2PortEntry {
    Interface(InterfaceAccess),
    Vxlan(VxlanAccess),
    Caps,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Plane {
    pub ports: Vec<L2Port>,
    pub vlan: Option<VlanAccess>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Setup {
    pub planes: Vec<L2Plane>,
}

impl L2Setup {
    pub fn new(device: DeviceAccess) -> Self {
        let mut planes = BTreeMap::<(InterfaceId, Option<VlanId>), L2Plane>::new();
        let mut vlans = HashMap::new();
        for interface in device.interfaces() {
            let option = interface.bridge();
            let bridge_id = option.map(|b| b.id()).unwrap_or(interface.id());
            let mut vlan_added = false;
            for (vlan, port) in interface
                .tagged_vlans()
                .map(|vlan| {
                    (
                        vlan,
                        L2Port {
                            entry: L2PortEntry::Interface(interface.clone()),
                            tagged: true,
                        },
                    )
                })
                .chain(interface.untagged_vlan().map(|vlan| {
                    (
                        vlan,
                        L2Port {
                            entry: L2PortEntry::Interface(interface.clone()),
                            tagged: false,
                        },
                    )
                }))
            {
                vlan_added = true;
                planes
                    .entry((bridge_id, Some(vlan.id)))
                    .or_default()
                    .ports
                    .push(port);
                vlans.insert(vlan.id, vlan);
            }
            if !vlan_added {
                planes
                    .entry((bridge_id, None))
                    .or_default()
                    .ports
                    .push(L2Port {
                        entry: L2PortEntry::Interface(interface.clone()),
                        tagged: false,
                    });
            }
        }
        L2Setup {
            planes: planes
                .into_iter()
                .map(|((_, vlan), mut plane)| {
                    plane.vlan = vlan.and_then(|id| vlans.get(&id).cloned());
                    plane
                })
                .collect(),
        }
    }
}
impl L2Plane {
    pub fn ips(&self) -> impl Iterator<Item = &IpNet> {
        self.ports
            .iter()
            .filter_map(|p| {
                if p.tagged {
                    None
                } else if let L2PortEntry::Interface(ifc) = &p.entry {
                    Some(ifc)
                } else {
                    None
                }
            })
            .flat_map(|ifc| ifc.ips().iter())
    }
}
