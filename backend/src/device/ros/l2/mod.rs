use crate::device::ros::SetupError;
use crate::topology::access::AccessTopology;
use crate::topology::access::ip_addresses::IpAddressAccess;
use crate::topology::access::vxlan::VxlanAccess;
use crate::topology::access::wlan::WlanAccess;
use crate::topology::{
    InterfaceId, PhysicalPortId, VlanId,
    access::{
        cable::CablePortAccess, device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess,
    },
};
use convert_case::{Case, Casing};
use ipnet::IpNet;
use mikrotik_model::ascii::AsciiString;
use std::collections::BTreeSet;
use std::collections::btree_set::Iter;
use std::iter::FilterMap;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
};

#[cfg(test)]
mod test;

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum L2Port {
    TaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    UntaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    VxLan {
        vxlan: VxlanAccess,
    },
    CapsMan {
        wlan: WlanAccess,
    },
    Cap {
        wlan: WlanAccess,
    },
    CapsManMgmt,
    CapMgmt,
    Ip(IpAddressAccess),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Plane {
    pub ports: BTreeSet<L2Port>,
    pub vlan: Option<VlanAccess>,
    pub vlan_id: u16,
    pub if_name: Option<AsciiString>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Setup<N: NameGenerator> {
    //pub planes: Vec<L2Plane>,
    //pub main_bridge_name: Option<AsciiString>,
    pub name_generator: N,
    pub single_ports: Box<[SingleInterfaceEntry]>,
    pub simple_bridges: Box<[SimpleBridge]>,
    pub tagged_bridges: Box<[TaggedBridge]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleInterfaceEntry {
    name: AsciiString,
    port: PhysicalPortId,
    root: InterfaceAccess,
    addresses: Box<[IpAddressAccess]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleBridgePort {
    Ethernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    Ip(IpAddressAccess),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleBridge {
    pub name: AsciiString,
    pub ports: BTreeSet<SimpleBridgePort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaggedBridgePort {
    TaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    UntaggedEthernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    Ip(IpAddressAccess),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedBridgeEntry {
    pub ports: BTreeSet<TaggedBridgePort>,
    pub vlan: Option<VlanAccess>,
    pub if_name: Option<AsciiString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedBridge {
    pub name: AsciiString,
    pub ports: BTreeMap<u16, TaggedBridgeEntry>,
    pub vxlan: Option<VxlanAccess>,
    pub wlan: Option<WlanAccess>,
    pub cap_mgmt: Option<u16>,
    pub capsman_mgmt: Option<u16>,
}

pub trait NameGenerator {
    fn generate_interface_name<'s>(&mut self, interface: &'s InterfaceAccess) -> Cow<'s, str>;
    fn generate_vxlan_name<'s>(&mut self, vxlan: &'s VxlanAccess) -> Cow<'s, str>;
    fn generate_vlan_name<'s>(
        &mut self,
        vlan: Option<&'s VlanAccess>,
        vlan_id: u16,
    ) -> Cow<'s, str>;
    fn generate_main_bridge<'s>(
        &mut self,
        root_device: Option<&'s InterfaceAccess>,
    ) -> Cow<'s, str>;
}
#[derive(Debug, Copy, Clone)]
pub struct KeepNameGenerator;
impl NameGenerator for KeepNameGenerator {
    fn generate_interface_name<'s>(&mut self, interface: &'s InterfaceAccess) -> Cow<'s, str> {
        Cow::Borrowed(interface.name())
    }

    fn generate_vxlan_name<'s>(&mut self, vxlan: &'s VxlanAccess) -> Cow<'s, str> {
        vxlan
            .name()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| format!("vxlan-{}", vxlan.id().0).into())
    }

    fn generate_vlan_name<'s>(
        &mut self,
        vlan: Option<&'s VlanAccess>,
        vlan_id: u16,
    ) -> Cow<'s, str> {
        format!("vlan-{}", vlan_id).into()
    }

    fn generate_main_bridge<'s>(
        &mut self,
        root_device: Option<&'s InterfaceAccess>,
    ) -> Cow<'s, str> {
        "bridge".into()
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

    fn generate_vxlan_name<'s>(&mut self, vxlan: &'s VxlanAccess) -> Cow<'s, str> {
        let mut wlan_group_names = HashSet::new();
        for vlan in vxlan.vlan_terminations() {
            for wlan in vlan.wlan() {
                let option = wlan.wlan_group();
                if let Some(group) = option.as_ref().and_then(|g| g.data()) {
                    if !wlan_group_names.contains(group.name.as_ref()) {
                        wlan_group_names.insert(group.name.clone());
                    }
                }
            }
        }
        if wlan_group_names.len() == 1 {
            if let Some(single_name) = wlan_group_names.iter().next() {
                return Cow::Owned(format!("vxlan-wlan-{}", single_name.to_case(Case::Kebab)));
            }
        }
        KeepNameGenerator.generate_vxlan_name(vxlan)
    }

    fn generate_vlan_name<'s>(
        &mut self,
        vlan: Option<&'s VlanAccess>,
        vlan_id: u16,
    ) -> Cow<'s, str> {
        vlan.and_then(|vlan| vlan.name())
            .map(|name| format!("vlan-{}-{vlan_id}", name.to_case(Case::Kebab)))
            .unwrap_or_else(|| format!("vlan-{vlan_id}"))
            .into()
    }

    fn generate_main_bridge<'s>(
        &mut self,
        root_device: Option<&'s InterfaceAccess>,
    ) -> Cow<'s, str> {
        root_device
            .map(|d| format!("bridge-{}", d.name().to_case(Case::Kebab)).into())
            .unwrap_or_else(|| "bridge".into())
    }
}

impl<G: NameGenerator> L2Setup<G> {
    pub fn new(device: &DeviceAccess, mut name_generator: G) -> Result<Self, SetupError> {
        let mut planes = BTreeMap::<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>::new();
        let mut vlans = HashMap::new();
        let mut root_devices = BTreeSet::new();
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
                                root: root_device.clone(),
                            },
                        )
                    })
                    .chain(interface.untagged_vlan().map(|vlan| {
                        (
                            vlan,
                            L2Port::UntaggedEthernet {
                                name: name.to_string().into(),
                                port,
                                root: root_device.clone(),
                            },
                        )
                    }))
                {
                    vlan_added = true;
                    planes
                        .entry((Some(bridge_id), Some(vlan.id)))
                        .or_insert_with(|| BTreeSet::new())
                        .insert(port);
                    vlans.insert(vlan.id, vlan);
                }
                if !vlan_added {
                    planes
                        .entry((Some(bridge_id), None))
                        .or_insert_with(|| BTreeSet::new())
                        .insert(L2Port::UntaggedEthernet {
                            name: name.to_string().into(),
                            port,
                            root: root_device.clone(),
                        });
                }
                for ip in interface.ips().into_iter() {
                    planes
                        .entry((Some(bridge_id), None))
                        .or_insert_with(|| BTreeSet::new())
                        .insert(L2Port::Ip(ip));
                }
            }
            root_devices.insert(root_device);
        }
        for vxlan in device.vxlan() {
            for vlan in vxlan.vlan_terminations().iter().map(|vlan| vlan.id()) {
                plane_of_vlan(&mut planes, vlan).insert(L2Port::VxLan {
                    vxlan: vxlan.clone(),
                });
            }
        }
        if let Some(wlan) = device.wlan_ap_of() {
            if let Some(vlan) = wlan.mgmt_vlan() {
                plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapMgmt);
            }
            for wlan in wlan.wlan() {
                if let Some(vlan) = wlan.vlan() {
                    plane_of_vlan(&mut planes, vlan.id).insert(L2Port::Cap { wlan });
                }
            }
        }
        if let Some(wlan) = device.wlan_controller_of() {
            if let Some(vlan) = wlan.mgmt_vlan() {
                plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapsManMgmt);
            }
            for wlan in wlan.wlan() {
                if let Some(vlan) = wlan.vlan() {
                    plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapsMan { wlan });
                }
            }
        }
        let used_vlans = planes
            .iter()
            .filter_map(|((_, vlan_id), plane)| {
                vlan_id
                    .as_ref()
                    .and_then(|vlan_id| vlans.get(vlan_id))
                    .and_then(|vlan| vlan.vlan_id())
                    .map(|vlan_id| (vlan_id, plane))
            })
            .fold(HashMap::new(), |mut map, (vlan_id, ports)| {
                map.entry(vlan_id).or_insert_with(Vec::new).push(ports);
                map
            });
        // check for redundant use of vlan ids
        let ambiguous_vlans = used_vlans
            .iter()
            .filter(|(_, planes)| planes.len() > 1)
            .map(|(id, planes)| (*id, planes.into_iter().cloned().cloned().collect()))
            .collect::<HashMap<_, _>>();
        if !ambiguous_vlans.is_empty() {
            return Err(SetupError::AmbiguousVlanIds(ambiguous_vlans));
        }
        todo!()
        /*for ((_, vlan), (ports)) in planes {}
        let mut used_vlans = used_vlans.keys().copied().collect::<HashSet<_>>();
        let planes = planes
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
                    if_name: None,
                }
            })
            .collect::<Vec<_>>();

        Ok(L2Setup {
            name_generator,
            single_ports: Box::new([]),
            simple_bridges: Box::new([]),
            tagged_bridges: Box::new([]),
        })*/
    }
    pub fn main_bridge_name(&mut self) -> AsciiString {
        {
            todo!();
            /*let all_interfaces = self
                .planes
                .iter()
                .flat_map(|plane| plane.interfaces())
                .collect::<BTreeSet<_>>();
            let root_device = if all_interfaces.len() == 1 {
                all_interfaces.into_iter().next()
            } else {
                None
            };
            AsciiString::from(
                self.name_generator
                    .generate_main_bridge(root_device)
                    .as_ref(),
            )*/
        }
    }

    pub fn can_create_single_plane(&self) -> Option<&L2Plane> {
        todo!()
        /*if self.main_bridge_name.is_none()
            && let [single_plane] = self.planes.as_slice()
        {
            if single_plane.vlan.is_none() {
                Some(single_plane)
            } else {
                None
            }
        } else {
            None
        }*/
    }
}

impl L2Plane {
    pub fn if_name<N: NameGenerator>(&mut self, name_generator: &mut N) -> &AsciiString {
        self.if_name.get_or_insert_with(|| {
            let all_interfaces =
                Self::filter_interface_access(self.ports.iter()).collect::<Vec<_>>();
            let n = if let [single_interface] = all_interfaces.as_slice() {
                name_generator.generate_interface_name(single_interface)
            } else {
                name_generator.generate_vlan_name(self.vlan.as_ref(), self.vlan_id)
            };
            AsciiString::from(n.as_ref())
        })
    }
    pub fn interfaces(&self) -> impl Iterator<Item = &InterfaceAccess> {
        Self::filter_interface_access(self.ports.iter())
    }

    fn filter_interface_access<'a>(
        iter: impl Iterator<Item = &'a L2Port>,
    ) -> impl Iterator<Item = &'a InterfaceAccess> {
        iter.filter_map(|p| match p {
            L2Port::TaggedEthernet { root, .. } => Some(root),
            L2Port::UntaggedEthernet { root, .. } => Some(root),
            _ => None,
        })
    }
    fn can_skip_bridge(&self) -> bool {
        if self
            .ports
            .iter()
            .map(|p| match p {
                L2Port::TaggedEthernet { .. } => 10,
                L2Port::UntaggedEthernet { .. } => 1,
                L2Port::VxLan { .. } => 10,
                L2Port::CapsMan { .. } => 10,
                L2Port::Cap { .. } => 10,
                L2Port::CapsManMgmt => 0,
                L2Port::CapMgmt => 0,
                L2Port::Ip(_) => 0,
            })
            .sum::<usize>()
            > 1
        {
            false
        } else {
            true
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
        Ordering::then_with(
            Option::cmp(
                &self.vlan.as_ref().map(|vlan| vlan.id),
                &other.vlan.as_ref().map(|vlan| vlan.id),
            ),
            || u16::cmp(&self.vlan_id, &other.vlan_id),
        )
    }
}

fn plane_of_vlan(
    planes: &mut BTreeMap<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>,
    vlan: VlanId,
) -> &mut BTreeSet<L2Port> {
    planes
        .entry((None, Some(vlan)))
        .or_insert_with(|| BTreeSet::new())
}
