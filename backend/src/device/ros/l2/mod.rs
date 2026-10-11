use crate::device::ros::{CAPS_BRIDGE_NAME, SetupError};
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
use ipnet::IpNet;
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
        name: AsciiString,
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
    /// An L3 address on this plane, together with the NetBox interface it is
    /// assigned to - whose tags (`ospf`/`dhcp`/`dhcp-client`) also apply to the
    /// logical interface the address ends up on.
    Ip {
        ip: IpAddressAccess,
        owner: InterfaceAccess,
    },
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

impl TaggedBridgeEntry {
    /// Whether the device itself is in this VLAN: an address on it, or a DHCP client from
    /// a tag of a member port's bridge. Otherwise it only switches the VLAN (an AP between
    /// its WLAN and the VXLAN) and needs no VLAN interface.
    pub fn has_l3(&self) -> bool {
        self.ports.iter().any(|port| match port {
            L2Port::Ip { .. } => true,
            L2Port::TaggedEthernet { root, .. } | L2Port::UntaggedEthernet { root, .. } => {
                root.is_enable_dhcp_client()
            }
            _ => false,
        })
    }
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
            parts.push(name_part(label));
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
                // Exactly one descriptor for the far end: the far device's name
                // (`e02-hegglin`, as in the reference exports), falling back to the far
                // port's short name and then its raw name only for an unnamed device -
                // not several combined, which used to produce redundant names like
                // `e02-hegglin-ether1`, and not the far port first, which produced
                // `e14-e01` for every link between two RouterOS devices.
                let far_descriptor = [
                    access.device().map(|device| device.name().to_string()),
                    match &access {
                        CablePortAccess::Interface(ifa) => {
                            ifa.external_port().map(|p| p.short_name().to_string())
                        }
                        _ => None,
                    },
                    access.name().map(str::to_string),
                ]
                .into_iter()
                .flatten()
                .map(|descriptor| name_part(&descriptor))
                .find(|descriptor| !descriptor.is_empty());
                parts.extend(far_descriptor);
            }
        }
        parts.retain(|part| !part.is_empty());
        if parts.is_empty() {
            parts.push(name_part(interface.name()));
        }
        parts.join("-").into()
    }

    /// `vxlan-{l2vpn-name}`, as in the reference exports (`vxlan-client-berg`).
    fn generate_vxlan_name<'s>(&mut self, vxlan: &'s VxlanAccess) -> Cow<'s, str> {
        vxlan
            .name()
            .map(name_part)
            .filter(|name| !name.is_empty())
            .map(|name| format!("vxlan-{name}").into())
            .unwrap_or_else(|| KeepNameGenerator.generate_vxlan_name(vxlan))
    }

    fn generate_vlan_name<'s>(
        &mut self,
        vlan: Option<&'s VlanAccess>,
        vlan_id: u16,
    ) -> Cow<'s, str> {
        vlan.and_then(|vlan| vlan.name())
            .map(name_part)
            .filter(|name| !name.is_empty())
            .map(|name| format!("vlan-{name}-{vlan_id}"))
            .unwrap_or_else(|| format!("vlan-{vlan_id}"))
            .into()
    }

    fn generate_main_bridge<'s>(
        &mut self,
        root_device: Option<&'s InterfaceAccess>,
    ) -> Cow<'s, str> {
        root_device
            .map(|d| name_part(d.name()))
            .filter(|name| !name.is_empty())
            .map(|name| format!("bridge-{name}").into())
            .unwrap_or_else(|| "bridge".into())
    }
}

impl<G: NameGenerator> L2Setup<G> {
    pub fn new(device: &DeviceAccess, mut name_generator: G) -> Result<Self, SetupError> {
        let mut planes = BTreeMap::<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>::new();
        let mut vlans = HashMap::new();
        let mut root_interfaces = HashMap::<InterfaceId, InterfaceAccess>::new();
        let referenced_bridges = device
            .interfaces()
            .iter()
            .filter_map(|interface| interface.bridge().map(|bridge| bridge.id()))
            .collect::<HashSet<_>>();
        for interface in device.interfaces() {
            let root_device = interface.bridge().unwrap_or(interface.clone());
            // An interface that carries a VLAN but names no bridge (and isn't one
            // itself) joins the device's default bridge - the root-less group, the
            // same one VXLAN/CAPsMAN/CAP entries land in - so a VLAN that only needs
            // L2 here (e.g. AP access ports in a VXLAN-transported household VLAN)
            // doesn't require modeling a bridge in NetBox at all.
            let carries_vlan =
                interface.untagged_vlan().is_some() || interface.tagged_vlans().next().is_some();
            let is_bridge = interface.is_bridge() || referenced_bridges.contains(&interface.id());
            let bridge_id = if interface.bridge().is_none() && carries_vlan && !is_bridge {
                None
            } else {
                Some(root_device.id())
            };
            if let Some(bridge_id) = bridge_id {
                root_interfaces
                    .entry(bridge_id)
                    .or_insert_with(|| root_device.clone());
            }
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
                        .entry((bridge_id, Some(vlan.id)))
                        .or_default()
                        .insert(port);
                    vlans.insert(vlan.id, vlan);
                }
                // A member port without a VLAN of its own is in its bridge's untagged
                // VLAN, if the bridge has one: a bridge in NetBox with an untagged VLAN
                // is a flat port group in that VLAN (e.g. a switch LAN in a household
                // VLAN), so a tunnel or gateway of that VLAN must reach these ports.
                let bridge_vlan = interface.bridge().and_then(|bridge| bridge.untagged_vlan());
                if !vlan_added && let Some(vlan) = bridge_vlan {
                    vlan_added = true;
                    planes
                        .entry((bridge_id, Some(vlan.id)))
                        .or_default()
                        .insert(L2Port::UntaggedEthernet {
                            name: name.to_string().into(),
                            port,
                            root: root_device.clone(),
                        });
                    vlans.insert(vlan.id, vlan);
                }
                if !vlan_added {
                    planes
                        .entry((bridge_id, None))
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
                    .entry((bridge_id, ip_vlan_id))
                    .or_default()
                    .insert(L2Port::Ip {
                        ip,
                        owner: interface.clone(),
                    });
            }
        }
        if let Some(wlan) = device.wlan_ap_of() {
            if let Some(vlan) = wlan.mgmt_vlan() {
                plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapMgmt);
                vlans.insert(vlan.id, vlan);
            }
            for wlan in wlan.wlan() {
                if let Some(vlan) = wlan.vlan() {
                    plane_of_vlan(&mut planes, vlan.id).insert(L2Port::Cap { wlan });
                    vlans.insert(vlan.id, vlan);
                }
            }
        }
        if let Some(wlan) = device.wlan_controller_of() {
            if let Some(vlan) = wlan.mgmt_vlan() {
                plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapsManMgmt);
                vlans.insert(vlan.id, vlan);
            }
            for wlan in wlan.wlan() {
                if let Some(vlan) = wlan.vlan() {
                    plane_of_vlan(&mut planes, vlan.id).insert(L2Port::CapsMan { wlan });
                    vlans.insert(vlan.id, vlan);
                }
            }
        }
        // Every VLAN used here that is also used on another device, in a VLAN group
        // with a VNI, reaches those devices through the group's VXLAN: a VLAN trunk,
        // so the VXLAN joins each such VLAN tagged. It goes root-less and so lands
        // wherever that VLAN is (an explicit bridge carrying it, or the default
        // bridge), like the CAP/CAPsMAN entries.
        for vlan in vlans.values() {
            check_l2_domain(device, vlan)?;
            if let Some(vxlan) = vlan.tunnel() {
                let name = AsciiString::from(name_generator.generate_vxlan_name(&vxlan).as_ref());
                plane_of_vlan(&mut planes, vlan.id).insert(L2Port::VxLan { name, vxlan });
            }
        }
        // Regroup by root device: each root ends up with its own map of
        // vlan -> ports, which the classification below turns into exactly one
        // single_ports/simple_bridges/tagged_bridges entry.
        let mut by_root =
            BTreeMap::<Option<InterfaceId>, BTreeMap<Option<VlanId>, BTreeSet<L2Port>>>::new();
        for ((root, vlan), ports) in planes {
            by_root.entry(root).or_default().insert(vlan, ports);
        }

        // Entries of the root-less group (VXLAN/CAPsMAN/CAP attachments from
        // `plane_of_vlan`, and interfaces carrying a VLAN without naming a bridge)
        // belong wherever that same NetBox VLAN is actually carried by an explicit
        // bridge, if any - merge them in. A VLAN with no explicit bridge keeps its
        // place in the root-less group, which becomes the device's default
        // `TaggedBridge` (see classification below). The untagged/no-VLAN entry is
        // never merged: it has no VLAN identity that could match another bridge.
        if let Some(virtual_group) = by_root.remove(&None) {
            for (vlan, virtual_ports) in virtual_group {
                let merged = vlan
                    .is_some()
                    .then(|| by_root.values_mut().find_map(|sub| sub.get_mut(&vlan)))
                    .flatten()
                    .map(|existing| existing.extend(virtual_ports.iter().cloned()));
                if merged.is_none() {
                    by_root.entry(None).or_default().insert(vlan, virtual_ports);
                }
            }
        }

        // Only now, after merging, can the same numeric VLAN ID still show up in
        // more than one place - the same NetBox VLAN on two explicit bridges, or two
        // different NetBox VLANs sharing an ID on one device. Also collects the
        // real IDs in use, to seed `next_synthetic_vlan_id` so it never collides.
        let mut planes_by_real_vlan_id = HashMap::<u16, Vec<BTreeSet<L2Port>>>::new();
        for sub in by_root.values() {
            for (vlan_id, ports) in sub {
                if let Some(numeric_id) = vlan_id
                    .as_ref()
                    .and_then(|vlan_id| vlans.get(vlan_id))
                    .and_then(VlanAccess::vlan_id)
                {
                    planes_by_real_vlan_id
                        .entry(numeric_id)
                        .or_default()
                        .push(ports.clone());
                }
            }
        }
        let used_real_vlan_ids = planes_by_real_vlan_id
            .keys()
            .copied()
            .collect::<HashSet<_>>();
        planes_by_real_vlan_id.retain(|_, planes| planes.len() > 1);
        if !planes_by_real_vlan_id.is_empty() {
            return Err(SetupError::AmbiguousVlanIds(planes_by_real_vlan_id));
        }

        // Merging is per VLAN, so a bridge-less trunk port could end up split
        // across an explicit bridge and the default one - but a port can only be a
        // member of one bridge.
        // The same goes for the VXLAN, which joins every tunneled VLAN: two of them
        // on different bridges would make it a member of both.
        let mut root_of_port = HashMap::<&AsciiString, Option<InterfaceId>>::new();
        for (root, sub) in &by_root {
            for port in sub.values().flatten() {
                if let L2Port::TaggedEthernet { name, .. }
                | L2Port::UntaggedEthernet { name, .. }
                | L2Port::VxLan { name, .. } = port
                {
                    if *root_of_port.entry(name).or_insert(*root) != *root {
                        return Err(SetupError::PortInMultipleBridges(name.clone()));
                    }
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
                        L2Port::UntaggedEthernet { name, port, root } => Some((name, *port, root)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let addresses = ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::Ip { ip, .. } => Some(ip.clone()),
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
                            L2Port::Ip { ip, .. } => Some(SimpleBridgePort::Ip(ip.clone())),
                            _ => None,
                        })
                        .collect();
                    simple_bridges.push(SimpleBridge {
                        name,
                        ports: bridge_ports,
                    });
                }
            } else {
                // On a WLAN AP the default bridge is the one the CAP datapath (or the
                // legacy CAP's `bridge`) references, so WLAN clients, the VXLAN and local
                // access ports of the same VLAN share one L2 domain.
                let name = if root.is_none() && device.wlan_ap_of().is_some() {
                    AsciiString::from(CAPS_BRIDGE_NAME.as_slice())
                } else {
                    AsciiString::from(
                        name_generator
                            .generate_main_bridge(root.and_then(|id| root_interfaces.get(&id)))
                            .as_ref(),
                    )
                };
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
            nat: s.root.is_nat(),
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
                nat: roots.iter().any(|r| r.is_nat()),
            }
        });
        let tagged = self.tagged_bridges.iter().flat_map(|b| {
            b.ports.values().map(|entry| {
                let addresses = entry
                    .ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::Ip { ip, .. } => Some(ip.clone()),
                        _ => None,
                    })
                    .collect();
                // Tags count from the bridge the member ports belong to and from
                // whichever interface owns an address on this VLAN - the latter is
                // the only place to put them for a VLAN on the default bridge, which
                // has no NetBox interface of its own.
                let roots = entry
                    .ports
                    .iter()
                    .filter_map(|p| match p {
                        L2Port::TaggedEthernet { root, .. }
                        | L2Port::UntaggedEthernet { root, .. } => Some(root),
                        L2Port::Ip { owner, .. } => Some(owner),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                LogicalInterface {
                    if_name: &entry.if_name,
                    addresses,
                    use_ospf: roots.iter().any(|r| r.use_ospf()),
                    enable_dhcp_client: roots.iter().any(|r| r.is_enable_dhcp_client()),
                    enable_dhcp_server: roots.iter().any(|r| r.is_enable_dhcp_server()),
                    nat: roots.iter().any(|r| r.is_nat()),
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
    pub nat: bool,
}

/// Turns free text from NetBox (labels, device and VLAN names) into one part of a
/// RouterOS interface name: lowercase, German umlauts transliterated (`Büro` ->
/// `buero`), every run of other characters outside `[a-z0-9]` (spaces, `/`, `.`,
/// accents...) a single `-`, no leading/trailing `-`. Interface names have to stay
/// ASCII, and a `/` would even be misread as a menu path in a RouterOS script.
/// Deliberately no case-based word splitting (`to_case`), which would also split
/// at digits and turn `1og-sw2` into `1-og-sw-2`.
fn name_part(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut pending_separator = false;
    let mut buf = [0u8; 4];
    for c in raw.chars().flat_map(char::to_lowercase) {
        let piece = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'ß' => "ss",
            c if c.is_ascii_alphanumeric() => c.encode_utf8(&mut buf),
            _ => {
                pending_separator = true;
                continue;
            }
        };
        if pending_separator && !result.is_empty() {
            result.push('-');
        }
        pending_separator = false;
        result.push_str(piece);
    }
    result
}

impl<G: NameGenerator> L2Setup<G> {
    /// The bridge a WLAN of this AP or controller is in: the one carrying its VLAN, where
    /// its clients join through the CAP's datapath (or, with the legacy CAPsMAN, the
    /// CAP's `bridge`). `None` for a WLAN without VLAN.
    pub fn bridge_of_wlan(&self, wlan: &WlanAccess) -> Option<&AsciiString> {
        self.tagged_bridges.iter().find_map(|bridge| {
            bridge
                .ports
                .values()
                .any(|entry| {
                    entry.ports.iter().any(|port| {
                        matches!(port, L2Port::Cap { wlan: w } | L2Port::CapsMan { wlan: w }
                            if w == wlan)
                    })
                })
                .then_some(&bridge.name)
        })
    }

    /// Where a physical port ended up: its generated interface name, and the bridge
    /// it is a member of (`None` for a standalone port).
    pub fn placement_of_port(
        &self,
        port: PhysicalPortId,
    ) -> Option<(&AsciiString, Option<&AsciiString>)> {
        let standalone = self
            .single_ports
            .iter()
            .find(|entry| entry.port == port)
            .map(|entry| (&entry.name, None));
        let in_simple_bridge = || {
            self.simple_bridges.iter().find_map(|bridge| {
                bridge.ports.iter().find_map(|member| match member {
                    SimpleBridgePort::Ethernet { name, port: p, .. } if *p == port => {
                        Some((name, Some(&bridge.name)))
                    }
                    _ => None,
                })
            })
        };
        let in_tagged_bridge = || {
            self.tagged_bridges.iter().find_map(|bridge| {
                bridge
                    .ports
                    .values()
                    .flat_map(|entry| &entry.ports)
                    .find_map(|member| match member {
                        L2Port::TaggedEthernet { name, port: p, .. }
                        | L2Port::UntaggedEthernet { name, port: p, .. }
                            if *p == port =>
                        {
                            Some((name, Some(&bridge.name)))
                        }
                        _ => None,
                    })
            })
        };
        standalone
            .or_else(in_simple_bridge)
            .or_else(in_tagged_bridge)
    }
}

/// Rejects a VLAN whose L2 domain - every device it is tunneled to, or just this
/// device for a local VLAN - would be broken: a loop through a cable next to the
/// tunnel, more than one IP network, or more than one DHCP server. Each of these
/// would only show up as an outage after provisioning, so nothing gets generated.
fn check_l2_domain(device: &DeviceAccess, vlan: &VlanAccess) -> Result<(), SetupError> {
    let vlan_name = || vlan.name().unwrap_or_default().into();
    let carries =
        |interface: &InterfaceAccess| interface.carried_vlans().iter().any(|v| v.id == vlan.id);
    let tunneled = vlan.tunnel().is_some();
    let domain = if tunneled {
        vlan.devices()
    } else {
        Box::new([device.clone()])
    };
    if tunneled {
        for interface in device.interfaces().iter().filter(|i| carries(i)) {
            for far in interface.connected_interfaces() {
                let far_device = far.device();
                let on_other_device = far_device.as_ref().is_some_and(|d| d.id() != device.id());
                if on_other_device && carries(&far) {
                    return Err(SetupError::VlanOnCableAndTunnel {
                        vlan: vlan_name(),
                        interface: interface.name().into(),
                        far_end: describe(&far),
                    });
                }
            }
        }
    }
    let mut networks = BTreeMap::<IpNet, Vec<String>>::new();
    let mut dhcp_servers = Vec::new();
    for member in domain.iter() {
        for interface in member.interfaces().iter() {
            if interface.untagged_vlan().is_none_or(|v| v.id != vlan.id) {
                continue;
            }
            for net in interface.ips().iter().filter_map(IpAddressAccess::net) {
                networks
                    .entry(net.trunc())
                    .or_default()
                    .push(describe(interface));
            }
            if interface.is_enable_dhcp_server() {
                dhcp_servers.push(describe(interface));
            }
        }
    }
    for family_is_v4 in [true, false] {
        let family = networks
            .iter()
            .filter(|(net, _)| matches!(net, IpNet::V4(_)) == family_is_v4)
            .map(|(net, owners)| format!("{net} ({})", owners.join(", ")))
            .collect::<Vec<_>>();
        if family.len() > 1 {
            return Err(SetupError::MultipleNetworksOnVlan {
                vlan: vlan_name(),
                networks: family,
            });
        }
    }
    if dhcp_servers.len() > 1 {
        return Err(SetupError::MultipleDhcpServersOnVlan {
            vlan: vlan_name(),
            servers: dhcp_servers,
        });
    }
    Ok(())
}

/// `device:interface`, for error messages.
fn describe(interface: &InterfaceAccess) -> String {
    let device = interface.device();
    let device_name = device.as_ref().map(|d| d.name()).unwrap_or_default();
    format!("{device_name}:{}", interface.name())
}

fn plane_of_vlan(
    planes: &mut BTreeMap<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>,
    vlan: VlanId,
) -> &mut BTreeSet<L2Port> {
    planes.entry((None, Some(vlan))).or_default()
}
