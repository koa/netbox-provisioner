use crate::topology::{
    InterfaceId, VlanId,
    access::{DeviceAccess, InterfaceAccess, VlanAccess},
};
use ipnet::IpNet;
use mikrotik_model::ascii::AsciiString;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
};

#[cfg(test)]
mod test;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L2Port {
    TaggedEthernet {
        name: AsciiString,
        default_name: AsciiString,
    },
    UntaggedEthernet {
        name: AsciiString,
        default_name: AsciiString,
    },
    VxLan {
        name: AsciiString,
    },
    Caps,
    L3 {
        access: L3Access,
        if_name: Option<AsciiString>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L3Access {
    Ip(IpNet),
    DhcpClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Plane {
    pub ports: Vec<L2Port>,
    pub vlan: Option<VlanAccess>,
    pub vlan_id: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Setup {
    pub planes: Vec<L2Plane>,
}

pub trait NameGenerator {
    fn generate_interface_name<'s>(&mut self, interface: &'s InterfaceAccess) -> Cow<'s, str>;
}
#[derive(Debug, Copy, Clone)]
pub struct KeepNameGenerator;
impl NameGenerator for KeepNameGenerator {
    fn generate_interface_name<'s>(&mut self, interface: &'s InterfaceAccess) -> Cow<'s, str> {
        Cow::Borrowed(interface.name())
    }
}

impl L2Setup {
    pub fn new<G: NameGenerator>(device: &DeviceAccess, name_generator: &mut G) -> Self {
        let mut planes = BTreeMap::<(InterfaceId, Option<VlanId>), Vec<L2Port>>::new();
        let mut vlans = HashMap::new();
        for interface in device.interfaces() {
            let option = interface.bridge();
            let bridge_id = option.map(|b| b.id()).unwrap_or(interface.id());
            let mut vlan_added = false;
            if let Some(default_name) = interface.external_port().and_then(|p| p.default_name()) {
                let name = name_generator.generate_interface_name(&interface);

                for (vlan, port) in interface
                    .tagged_vlans()
                    .map(|vlan| {
                        (
                            vlan,
                            L2Port::TaggedEthernet {
                                name: name.to_string().into(),
                                default_name: default_name.clone(),
                            },
                        )
                    })
                    .chain(interface.untagged_vlan().map(|vlan| {
                        (
                            vlan,
                            L2Port::UntaggedEthernet {
                                name: name.to_string().into(),
                                default_name: default_name.clone(),
                            },
                        )
                    }))
                {
                    vlan_added = true;
                    planes
                        .entry((bridge_id, Some(vlan.id)))
                        .or_default()
                        .push(port);
                    vlans.insert(vlan.id, vlan);
                }
                if !vlan_added {
                    planes
                        .entry((bridge_id, None))
                        .or_default()
                        .push(L2Port::UntaggedEthernet {
                            name: name.to_string().into(),
                            default_name: default_name.clone(),
                        });
                }
            }
            if interface.tagged_vlans().next().is_none() {
                let tag = interface.untagged_vlan().map(|v| v.id);
                let ips = interface.ips();
                for access in ips.iter().map(|ip| L3Access::Ip(*ip)).chain(
                    if ips.is_empty() && interface.enable_dhcp_client() {
                        Some(L3Access::DhcpClient)
                    } else {
                        None
                    },
                ) {
                    planes
                        .entry((bridge_id, tag))
                        .or_default()
                        .push(L2Port::L3 {
                            access,
                            if_name: Some(interface.name().into()),
                        });
                }
            }
        }
        let mut used_vlans = planes
            .keys()
            .filter_map(|(_, v)| v.as_ref())
            .filter_map(|id| vlans.get(id))
            .filter_map(|vlan| vlan.vlan_id())
            .collect::<HashSet<_>>();
        L2Setup {
            planes: planes
                .into_iter()
                .map(|((_, vlan), ports)| {
                    let vlan = vlan.and_then(|id| vlans.get(&id).cloned());
                    let vlan_id = if let Some(vlan_id) = vlan.as_ref().and_then(|vl| vl.vlan_id()) {
                        vlan_id
                    } else {
                        let mut candidate = 60000;
                        while !used_vlans.insert(candidate) {
                            candidate += 1;
                        }
                        candidate
                    };
                    L2Plane {
                        ports,
                        vlan,
                        vlan_id,
                    }
                })
                .collect(),
        }
    }
}
impl L2Plane {
    pub fn ips(&self) -> impl Iterator<Item = &IpNet> {
        self.ports.iter().filter_map(|p| {
            if let L2Port::L3 {
                access: L3Access::Ip(ip),
                ..
            } = &p
            {
                Some(ip)
            } else {
                None
            }
        })
    }
    pub fn need_tag(&self) -> bool {
        self.vlan
            .as_ref()
            .map(|vlan| vlan.vlan_id().is_none())
            .unwrap_or(false)
            && self.ports.iter().any(|p| match p {
                L2Port::TaggedEthernet { .. } => true,
                L2Port::UntaggedEthernet { .. } => false,
                L2Port::VxLan { .. } => true,
                L2Port::Caps => true,
                L2Port::L3 { .. } => false,
            })
    }
}
