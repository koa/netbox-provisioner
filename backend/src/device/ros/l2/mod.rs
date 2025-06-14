use crate::topology::{
    InterfaceId, PhysicalPortId, VlanId,
    access::{
        cable::CablePortAccess, device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess,
    },
};
use convert_case::{Case, Casing};
use mikrotik_model::ascii::AsciiString;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
};

#[cfg(test)]
mod test;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L2Port {
    TaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
    },
    UntaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
    },
    VxLan {
        name: AsciiString,
    },
    Caps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Plane {
    pub ports: Vec<L2Port>,
    pub vlan: Option<VlanAccess>,
    pub vlan_id: u16,
    pub root_port: InterfaceAccess,
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
#[derive(Debug, Copy, Clone)]
pub struct EndpointNameGenerator;

impl NameGenerator for EndpointNameGenerator {
    fn generate_interface_name<'s>(&mut self, interface: &'s InterfaceAccess) -> Cow<'s, str> {
        enum WalkResult {
            None,
            Single(CablePortAccess),
            Multiple,
        }
        let mut walk_result = WalkResult::None;
        let cable_port_access = interface.cable_port();
        cable_port_access.walk_cable(
            &mut (|path| {
                let far_port = path.far_port();
                if far_port != &cable_port_access {
                    match &walk_result {
                        WalkResult::None => {
                            walk_result = WalkResult::Single(far_port.clone());
                        }
                        WalkResult::Single(access) if access == far_port => {}
                        _ => walk_result = WalkResult::Multiple,
                    }
                }
            }),
        );
        let mut parts = Vec::new();
        if let Some(port) = interface.external_port() {
            parts.push(port.short_name().to_string());
        }
        if let WalkResult::Single(access) = walk_result {
            if let Some(device) = access.device() {
                parts.push(device.name().to_case(Case::Kebab));
            }
            let mut short_name_added = false;
            if let CablePortAccess::Interface(ifa) = &access {
                if let Some(p) = ifa.external_port() {
                    parts.push(p.short_name().to_string());
                    short_name_added = true;
                }
            }
            if !short_name_added {
                if let Some(p_name) = access.name().map(|s| s.to_case(Case::Kebab)) {
                    parts.push(p_name);
                }
            }
        }
        if parts.is_empty() {
            parts.push(interface.name().to_case(Case::Kebab));
        }
        parts.join("-").into()
    }
}

impl L2Setup {
    pub fn new<G: NameGenerator>(device: &DeviceAccess, name_generator: &mut G) -> Self {
        let mut planes =
            BTreeMap::<(InterfaceId, Option<VlanId>), (InterfaceAccess, Vec<L2Port>)>::new();
        let mut vlans = HashMap::new();
        for interface in device.interfaces() {
            let root_device = interface.bridge().unwrap_or(interface.clone());
            let bridge_id = root_device.id();
            let mut vlan_added = false;
            if let Some(port) = interface.external_port() {
                let name = name_generator.generate_interface_name(&interface);

                for (vlan, port) in interface
                    .tagged_vlans()
                    .map(|vlan| {
                        (
                            vlan,
                            L2Port::TaggedEthernet {
                                name: name.to_string().into(),
                                port,
                            },
                        )
                    })
                    .chain(interface.untagged_vlan().map(|vlan| {
                        (
                            vlan,
                            L2Port::UntaggedEthernet {
                                name: name.to_string().into(),
                                port,
                            },
                        )
                    }))
                {
                    vlan_added = true;
                    planes
                        .entry((bridge_id, Some(vlan.id)))
                        .or_insert_with(|| (root_device.clone(), Vec::new()))
                        .1
                        .push(port);
                    vlans.insert(vlan.id, vlan);
                }
                if !vlan_added {
                    planes
                        .entry((bridge_id, None))
                        .or_insert_with(|| (root_device.clone(), Vec::new()))
                        .1
                        .push(L2Port::UntaggedEthernet {
                            name: name.to_string().into(),
                            port,
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
                .map(|((_, vlan), (root_port, ports))| {
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
                        root_port,
                    }
                })
                .collect(),
        }
    }
}
impl PartialOrd for L2Plane {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for L2Plane {
    fn cmp(&self, other: &Self) -> Ordering {
        self.vlan
            .as_ref()
            .map(|vlan| vlan.id)
            .cmp(&other.vlan.as_ref().map(|vlan| vlan.id))
            .then_with(|| self.root_port.id().cmp(&other.root_port.id()))
            .then_with(|| self.vlan_id.cmp(&other.vlan_id))
    }
}
