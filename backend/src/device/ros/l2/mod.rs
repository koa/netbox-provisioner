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
use mikrotik_model::ascii::AsciiString;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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

/// A device's full L2 setup, split into three independent kinds of L2 domains rather
/// than one flat list: a domain only needs the generality (and RouterOS config cost)
/// of a VLAN-filtering bridge once it actually carries more than one VLAN or a
/// VXLAN/CAPsMAN attachment (see `L2Setup::new`'s classification rule) - most domains
/// in practice are much simpler than that.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L2Setup<N: NameGenerator> {
    pub name_generator: N,
    pub single_ports: Box<[SingleInterfaceEntry]>,
    pub simple_bridges: Box<[SimpleBridge]>,
    pub tagged_bridges: Box<[TaggedBridge]>,
}

/// One physical port used standalone, with no RouterOS bridge interface created for
/// it at all (there is nothing to bridge together).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleInterfaceEntry {
    pub name: AsciiString,
    pub port: PhysicalPortId,
    pub root: InterfaceAccess,
    pub addresses: Box<[IpAddressAccess]>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimpleBridgePort {
    Ethernet {
        name: AsciiString,
        port: PhysicalPortId,
        root: InterfaceAccess,
    },
    Ip(IpAddressAccess),
}

/// A RouterOS bridge without VLAN filtering, combining multiple ports into one flat
/// L2 domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleBridge {
    pub name: AsciiString,
    pub ports: BTreeSet<SimpleBridgePort>,
}

/// One VLAN's worth of ports on a `TaggedBridge`. Reuses `L2Port` directly (rather
/// than a separate per-context port enum) since a VLAN entry can legitimately carry
/// any of `L2Port`'s variants - including `VxLan`/`CapsMan`/`Cap`/`*Mgmt`, which is
/// exactly the per-household-VLAN VXLAN transport and WLAN mgmt-VLAN pattern this
/// network uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedBridgeEntry {
    pub ports: BTreeSet<L2Port>,
    pub vlan: Option<VlanAccess>,
    pub if_name: AsciiString,
}

/// A VLAN-filtering RouterOS bridge, keyed by numeric VLAN ID. A key's VLAN ID is
/// either the real NetBox-assigned one, or - for a VLAN entry with no numeric ID yet,
/// or the "untagged, no VLAN" default entry - a synthetic one assigned by
/// `L2Setup::new` (see its `next_synthetic_vlan_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedBridge {
    pub name: AsciiString,
    pub ports: BTreeMap<u16, TaggedBridgeEntry>,
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
        let mut parts = Vec::new();
        if let Some(port) = interface.external_port() {
            parts.push(port.short_name().to_string());
        }
        // A human-entered label on the interface itself is the strongest signal of
        // intent (e.g. "AP Keller", "Bachmann Dachboden") and is how most
        // customer/household links are documented in practice - NetBox rarely
        // models a separate cabled far-end device for those. Only fall back to
        // describing the cable's far end (useful for infrastructure links between
        // NetBox-modeled devices, which are typically left unlabeled) when no label
        // was given.
        if let Some(label) = interface.label() {
            parts.push(label.to_case(Case::Kebab));
        } else {
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
            if let WalkResult::Single(access) = walk_result {
                // Exactly one descriptor for the far end, most specific first - not
                // "device name, plus also whatever else we can find", which used to
                // produce redundant three-part names like `e02-hegglin-ether1`
                // instead of the intended `e02-hegglin`.
                let mut far_descriptor_added = false;
                if let CablePortAccess::Interface(ifa) = &access {
                    if let Some(p) = ifa.external_port() {
                        parts.push(p.short_name().to_string());
                        far_descriptor_added = true;
                    }
                }
                if !far_descriptor_added {
                    if let Some(device) = access.device() {
                        parts.push(device.name().to_case(Case::Kebab));
                        far_descriptor_added = true;
                    }
                }
                if !far_descriptor_added {
                    if let Some(p_name) = access.name().map(|s| s.to_case(Case::Kebab)) {
                        parts.push(p_name);
                    }
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
        let mut root_interfaces = HashMap::<InterfaceId, InterfaceAccess>::new();
        for interface in device.interfaces() {
            let root_device = interface.bridge().unwrap_or(interface.clone());
            let bridge_id = root_device.id();
            root_interfaces
                .entry(bridge_id)
                .or_insert_with(|| root_device.clone());
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
                        .or_default()
                        .insert(port);
                    vlans.insert(vlan.id, vlan);
                }
                if !vlan_added {
                    planes
                        .entry((Some(bridge_id), None))
                        .or_default()
                        .insert(L2Port::UntaggedEthernet {
                            name: name.to_string().into(),
                            port,
                            root: root_device.clone(),
                        });
                }
            }
            // An interface's own IP addresses belong on whichever VLAN it itself
            // represents - its `untagged_vlan()`, same as for a physical access port
            // above - not unconditionally on the no-VLAN bucket. This is what lets a
            // NetBox-modeled "VLAN interface" (a virtual interface with no external
            // port, just a `vlan` and `ips`) carry that VLAN's L3 address.
            let ip_vlan = interface.untagged_vlan();
            let ip_vlan_id = ip_vlan.as_ref().map(|vlan| vlan.id);
            if let Some(vlan) = ip_vlan {
                vlans.insert(vlan.id, vlan);
            }
            for ip in interface.ips().into_iter() {
                planes
                    .entry((Some(bridge_id), ip_vlan_id))
                    .or_default()
                    .insert(L2Port::Ip(ip));
            }
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
        // Group plane keys (not their contents - that would keep `planes` borrowed
        // across the move into `by_root` below) by the real numeric VLAN ID they
        // resolve to, to catch the same NetBox VLAN being used in two unrelated
        // places, and to seed `next_synthetic_vlan_id` so it never collides with a
        // real ID.
        let mut keys_by_real_vlan_id = HashMap::<u16, Vec<(Option<InterfaceId>, Option<VlanId>)>>::new();
        for key @ (_, vlan_id) in planes.keys() {
            if let Some(numeric_id) = vlan_id
                .as_ref()
                .and_then(|vlan_id| vlans.get(vlan_id))
                .and_then(VlanAccess::vlan_id)
            {
                keys_by_real_vlan_id.entry(numeric_id).or_default().push(*key);
            }
        }
        let ambiguous_vlans = keys_by_real_vlan_id
            .iter()
            .filter(|(_, keys)| keys.len() > 1)
            .map(|(id, keys)| {
                (
                    *id,
                    keys.iter().map(|key| planes[key].clone()).collect(),
                )
            })
            .collect::<HashMap<_, _>>();
        if !ambiguous_vlans.is_empty() {
            return Err(SetupError::AmbiguousVlanIds(ambiguous_vlans));
        }
        let used_real_vlan_ids = keys_by_real_vlan_id.keys().copied().collect::<HashSet<_>>();

        // Regroup by root device: each root ends up with its own map of
        // vlan -> ports, which `classify_root_group` below turns into exactly one
        // single_ports/simple_bridges/tagged_bridges entry.
        let mut by_root =
            BTreeMap::<Option<InterfaceId>, BTreeMap<Option<VlanId>, BTreeSet<L2Port>>>::new();
        for ((root, vlan), ports) in planes {
            by_root.entry(root).or_default().insert(vlan, ports);
        }

        // VXLAN/CAPsMAN/CAP-only entries (`plane_of_vlan` above has no root device to
        // key them under) belong wherever that same NetBox VLAN is actually carried by
        // a physical bridge group, if any - merge them in. A VLAN that is only ever
        // reached via VXLAN/WLAN, with no local tagged/untagged uplink, keeps its own
        // root-less group and still becomes a `TaggedBridge` (see classification
        // below), just one with no physical member port.
        if let Some(virtual_group) = by_root.remove(&None) {
            for (vlan, virtual_ports) in virtual_group {
                let merged = by_root
                    .values_mut()
                    .find_map(|sub| sub.get_mut(&vlan))
                    .map(|existing| existing.extend(virtual_ports.iter().cloned()));
                if merged.is_none() {
                    by_root.entry(None).or_default().insert(vlan, virtual_ports);
                }
            }
        }

        let mut next_synthetic_vlan_id = {
            let mut used = used_real_vlan_ids;
            move || {
                let mut candidate = 60000;
                while !used.insert(candidate) {
                    candidate += 1;
                }
                candidate
            }
        };

        let mut single_ports = Vec::new();
        let mut simple_bridges = Vec::new();
        let mut tagged_bridges = Vec::new();
        for (root, sub) in by_root {
            let has_special_port = sub.values().flatten().any(|p| {
                matches!(
                    p,
                    L2Port::VxLan { .. }
                        | L2Port::CapsMan { .. }
                        | L2Port::Cap { .. }
                        | L2Port::CapsManMgmt
                        | L2Port::CapMgmt
                )
            });
            let is_flat = sub.len() == 1 && sub.contains_key(&None);
            if root.is_some() && is_flat && !has_special_port {
                let ports = sub.into_values().next().expect("checked len == 1 above");
                let untagged = ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::UntaggedEthernet { name, port, root } => {
                            Some((name, *port, root))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let addresses = ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::Ip(ip) => Some(ip.clone()),
                        _ => None,
                    })
                    .collect::<Box<[_]>>();
                if let [(name, port, port_root)] = untagged.as_slice() {
                    single_ports.push(SingleInterfaceEntry {
                        name: (*name).clone(),
                        port: *port,
                        root: (*port_root).clone(),
                        addresses,
                    });
                } else {
                    let name = AsciiString::from(
                        name_generator
                            .generate_main_bridge(root.and_then(|id| root_interfaces.get(&id)))
                            .as_ref(),
                    );
                    let bridge_ports = ports
                        .iter()
                        .filter_map(|p| match p {
                            L2Port::UntaggedEthernet { name, port, root } => {
                                Some(SimpleBridgePort::Ethernet {
                                    name: name.clone(),
                                    port: *port,
                                    root: root.clone(),
                                })
                            }
                            L2Port::Ip(ip) => Some(SimpleBridgePort::Ip(ip.clone())),
                            _ => None,
                        })
                        .collect();
                    simple_bridges.push(SimpleBridge {
                        name,
                        ports: bridge_ports,
                    });
                }
            } else {
                let name = AsciiString::from(
                    name_generator
                        .generate_main_bridge(root.and_then(|id| root_interfaces.get(&id)))
                        .as_ref(),
                );
                let mut entries = BTreeMap::new();
                for (vlan_id, ports) in sub {
                    let vlan = vlan_id.and_then(|id| vlans.get(&id).cloned());
                    let numeric_vlan_id = vlan
                        .as_ref()
                        .and_then(VlanAccess::vlan_id)
                        .unwrap_or_else(&mut next_synthetic_vlan_id);
                    // A bridge member port never carries its own IP once enslaved, so
                    // the L3 presence for a VLAN always sits on the bridge itself (the
                    // "no VLAN"/native entry) or on a dedicated `/interface/vlan`
                    // sub-interface (every other entry) - never on a member port's own
                    // name, regardless of how many physical ports carry that VLAN.
                    let if_name = if vlan_id.is_none() {
                        name.clone()
                    } else {
                        AsciiString::from(
                            name_generator
                                .generate_vlan_name(vlan.as_ref(), numeric_vlan_id)
                                .as_ref(),
                        )
                    };
                    entries.insert(
                        numeric_vlan_id,
                        TaggedBridgeEntry {
                            ports,
                            vlan,
                            if_name,
                        },
                    );
                }
                tagged_bridges.push(TaggedBridge {
                    name,
                    ports: entries,
                });
            }
        }

        Ok(L2Setup {
            name_generator,
            single_ports: single_ports.into_boxed_slice(),
            simple_bridges: simple_bridges.into_boxed_slice(),
            tagged_bridges: tagged_bridges.into_boxed_slice(),
        })
    }

    /// A unified view over every logical L3 interface this device ends up with,
    /// regardless of which of the three L2 domain kinds it came from - used by
    /// IP/DHCP and OSPF setup, which don't care about that distinction.
    pub fn logical_interfaces(&self) -> impl Iterator<Item = LogicalInterface<'_>> {
        let singles = self.single_ports.iter().map(|s| LogicalInterface {
            if_name: &s.name,
            addresses: s.addresses.clone(),
            use_ospf: s.root.use_ospf(),
            enable_dhcp_client: s.root.is_enable_dhcp_client(),
            enable_dhcp_server: s.root.is_enable_dhcp_server(),
        });
        let simple = self.simple_bridges.iter().map(|b| {
            let addresses = b
                .ports
                .iter()
                .filter_map(|p| match p {
                    SimpleBridgePort::Ip(ip) => Some(ip.clone()),
                    _ => None,
                })
                .collect();
            let roots = b
                .ports
                .iter()
                .filter_map(|p| match p {
                    SimpleBridgePort::Ethernet { root, .. } => Some(root),
                    _ => None,
                })
                .collect::<Vec<_>>();
            LogicalInterface {
                if_name: &b.name,
                addresses,
                use_ospf: roots.iter().any(|r| r.use_ospf()),
                enable_dhcp_client: roots.iter().any(|r| r.is_enable_dhcp_client()),
                enable_dhcp_server: roots.iter().any(|r| r.is_enable_dhcp_server()),
            }
        });
        let tagged = self.tagged_bridges.iter().flat_map(|b| {
            b.ports.values().map(|entry| {
                let addresses = entry
                    .ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::Ip(ip) => Some(ip.clone()),
                        _ => None,
                    })
                    .collect();
                let roots = entry
                    .ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::TaggedEthernet { root, .. } | L2Port::UntaggedEthernet { root, .. } => {
                            Some(root)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                LogicalInterface {
                    if_name: &entry.if_name,
                    addresses,
                    use_ospf: roots.iter().any(|r| r.use_ospf()),
                    enable_dhcp_client: roots.iter().any(|r| r.is_enable_dhcp_client()),
                    enable_dhcp_server: roots.iter().any(|r| r.is_enable_dhcp_server()),
                }
            })
        });
        singles.chain(simple).chain(tagged)
    }
}

/// One logical L3-capable interface (a standalone port, a flat bridge, or one VLAN of
/// a tagged bridge), as seen by `L2Setup::logical_interfaces`.
pub struct LogicalInterface<'a> {
    pub if_name: &'a AsciiString,
    pub addresses: Box<[IpAddressAccess]>,
    pub use_ospf: bool,
    pub enable_dhcp_client: bool,
    pub enable_dhcp_server: bool,
}

fn plane_of_vlan(
    planes: &mut BTreeMap<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>,
    vlan: VlanId,
) -> &mut BTreeSet<L2Port> {
    planes
        .entry((None, Some(vlan)))
        .or_insert_with(|| BTreeSet::new())
}
