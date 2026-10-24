//! The interfaces of a device by their task (`docs/concepts/interface-view.md`): loopback,
//! uplink, direct link, radio link, customer port, unknown. Each row is an L3 interface, a
//! bridge with its ports in the same row, with the findings about it; the findings are the
//! ones of `findings.rs`, grouped, not computed again. Computed from NetBox alone; the
//! frontend only shows it.
use crate::topology::{
    DeviceRole, InterfaceId, Topology,
    access::{
        AccessTopology, device::DeviceAccess, interface::InterfaceAccess,
        ip_addresses::IpAddressAccess, vlan::VlanAccess,
    },
    findings::{
        AddressRef, DeviceRef, Finding, FindingKind, InterfaceRef, VlanRef, address_ref,
        device_ref, interface_ref, vlan_ref,
    },
    natural::naturalize_interface,
    vlan_domain,
};
use async_graphql::{Enum, SimpleObject};
use std::{collections::BTreeSet, sync::Arc};

/// What an interface is for. Checked in this order, the first that fits counts: loopback,
/// uplink, a radio of a wireless link (radio link), an untagged VLAN or `dhcp` (customer),
/// direct, a cable to a radio bridge device (radio link), an address in a prefix of no pool
/// (customer), unknown.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Enum)]
pub enum InterfaceTask {
    /// `lo`
    Loopback,
    /// the tag `dhcp-client` or `nat`: modeled in NetBox only, no helper
    Uplink,
    /// a cable to another RouterOS device that is no radio bridge device
    Direct,
    /// an end of a wireless link (with its bridge, so on a bridge device also the port to the
    /// router), or a cable to a radio bridge device
    RadioLink,
    /// an untagged VLAN, an address in a prefix of no pool, or the tag `dhcp`
    Customer,
    /// anything else, also a free port
    Unknown,
}

/// The interfaces of a device by task, in NetBox' order of names.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceView {
    pub rows: Vec<InterfaceRow>,
    /// the findings about the device as a whole (no `lo`, the primary IPv4): they belong
    /// with the loopback
    pub device_findings: Vec<Finding>,
    /// the findings of the device on none of its interfaces (a WLAN of its group)
    pub other_findings: Vec<Finding>,
    /// what the helpers can do for the loopback (also where there is none)
    pub loopback_actions: Vec<RowAction>,
}

/// What a button of a row does: complete what is there, take the link out of NetBox, or give
/// an unknown interface a task.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum ActionKind {
    Suggestion,
    Remove,
    AssignRole,
}

/// The helper a row's action opens (`helper::Helper`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum ActionHelper {
    Loopback,
    Connection,
    Disconnect,
    CustomerNet,
    RadioBridge,
    /// „Richtfunk abräumen“, from a radio of the link (`interface`)
    RadioDisconnect,
    /// „Kunden-Anschluss abräumen“ of the row's interface
    CustomerDisconnect,
    /// „VLAN am Port“: a VLAN untagged on a port (`interface`, `vlan`, either may be open)
    PortVlan,
}

/// An action of a row: the helper and what it starts with. The frontend hands it on to the
/// helper's form; with all it needs the plan is there at once.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RowAction {
    pub kind: ActionKind,
    pub helper: ActionHelper,
    /// the device's port: the end of a connection, the link to take out, a customer port
    pub interface: Option<u32>,
    /// the other end of a connection
    pub peer: Option<InterfaceRef>,
    /// a customer network on this VLAN
    pub vlan: Option<u32>,
}

/// An L3 interface, a bridge with its ports.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceRow {
    pub task: InterfaceTask,
    pub interface: InterfaceRef,
    pub label: Option<Box<str>>,
    /// the ports of a bridge
    pub ports: Vec<InterfaceRef>,
    pub addresses: Vec<AddressRef>,
    pub ospf: bool,
    pub dhcp: bool,
    pub dhcp_client: bool,
    pub nat: bool,
    pub mtu: Option<u16>,
    pub untagged_vlan: Option<VlanRef>,
    pub tagged_vlans: Vec<VlanRef>,
    /// the interfaces its cables (or its ports' cables) end at, on other devices
    pub far_ends: Vec<InterfaceRef>,
    /// a radio link: the way to the router on the other side
    pub radio_path: Option<RadioPath>,
    /// a customer port without an address here: the devices with an address on its VLAN
    pub gateways: Vec<DeviceRef>,
    /// no cable, no address, no VLAN, no tag, no label: nothing in NetBox
    pub free: bool,
    /// the worst first
    pub findings: Vec<Finding>,
    /// the helpers it offers, the suggestion first
    pub actions: Vec<RowAction>,
}

/// A radio link from a router port: `router_port ── bridge_port ┄ link ┄ peer_bridge_port ──
/// peer_router_port`. What is missing in NetBox stays empty.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct RadioPath {
    pub router_port: Option<InterfaceRef>,
    /// the bridge device's port to the router
    pub bridge_port: Option<InterfaceRef>,
    /// its radio
    pub radio: Option<InterfaceRef>,
    /// the SSID of the wireless link
    pub ssid: Option<Box<str>>,
    pub peer_radio: Option<InterfaceRef>,
    pub peer_bridge_port: Option<InterfaceRef>,
    pub peer_router_port: Option<InterfaceRef>,
}

impl DeviceAccess {
    /// A radio bridge device: role `wlan-bridge`, or exactly one radio that has an RF role or a
    /// wireless link (the lab's have role `router`, many link ends of the stored export no RF
    /// role).
    pub fn is_bridge_device(&self) -> bool {
        self.role() == Some(DeviceRole::WlanBridge)
            || self
                .interfaces()
                .iter()
                .filter(|i| i.radio().is_some() || i.has_wireless_link())
                .count()
                == 1
    }

    /// Its rows without findings and actions, each with the NetBox ids of its interface and
    /// bridge ports (what the live checks group by).
    pub fn interface_rows(&self) -> Vec<(InterfaceRow, BTreeSet<u32>)> {
        let mut interfaces = self.interfaces().into_vec();
        interfaces.sort_by_cached_key(|i| naturalize_interface(i.name()));
        interfaces
            .iter()
            .filter(|interface| interface.bridge().is_none())
            .map(|interface| {
                let ports = interfaces
                    .iter()
                    .filter(|port| port.bridge().as_ref() == Some(interface))
                    .cloned()
                    .collect::<Vec<_>>();
                row(interface, &ports)
            })
            .collect()
    }

    /// Its interfaces by task.
    pub fn interface_view(&self) -> InterfaceView {
        let mut rows = self.interface_rows();
        let mut device_findings = Vec::new();
        let mut other_findings = Vec::new();
        for finding in self
            .topology()
            .findings()
            .iter()
            .filter(|finding| finding.devices.contains(&self.id()))
        {
            if finding.kind.is_about_device() {
                device_findings.push(finding.clone());
                continue;
            }
            let concerned = finding.kind.interfaces();
            let mut placed = false;
            for (row, members) in rows.iter_mut() {
                if members.iter().any(|id| concerned.contains(id)) {
                    row.findings.push(finding.clone());
                    placed = true;
                }
            }
            if !placed {
                other_findings.push(finding.clone());
            }
        }
        let topology = self.topology();
        let bridge_device = self.is_bridge_device();
        for (row, _) in rows.iter_mut() {
            row.actions = actions(topology, row, bridge_device);
        }
        let loopback = rows.iter().find(|(row, _)| row.task == InterfaceTask::Loopback);
        let loopback_missing = loopback.is_none_or(|(row, _)| {
            !row.addresses.iter().any(|a| !a.address.contains(':'))
        }) || device_findings.iter().any(|f| {
            matches!(
                f.kind,
                FindingKind::NoLoopback(_)
                    | FindingKind::NoPrimaryIpv4(_)
                    | FindingKind::PrimaryIpv4NotOnLoopback(_)
            )
        });
        let loopback_actions = loopback_missing
            .then_some(RowAction {
                kind: ActionKind::Suggestion,
                helper: ActionHelper::Loopback,
                interface: None,
                peer: None,
                vlan: None,
            })
            .into_iter()
            .collect();
        InterfaceView {
            rows: rows.into_iter().map(|(row, _)| row).collect(),
            device_findings,
            other_findings,
            loopback_actions,
        }
    }
}

/// The row of `interface` with its bridge `ports`, and the NetBox ids of all of them.
fn row(interface: &InterfaceAccess, ports: &[InterfaceAccess]) -> (InterfaceRow, BTreeSet<u32>) {
    let members = std::iter::once(interface).chain(ports).collect::<Vec<_>>();
    let far_ends = members
        .iter()
        .flat_map(|member| member.connected_interfaces())
        .filter(|far| far.device().as_ref() != interface.device().as_ref())
        .collect::<Vec<_>>();
    let routeros_far = far_ends
        .iter()
        .filter_map(|far| far.device().filter(DeviceAccess::has_routeros))
        .collect::<Vec<_>>();
    let radio = members.iter().find(|member| member.has_wireless_link());
    let addresses = interface.ips();
    let task = if interface.name() == "lo" {
        InterfaceTask::Loopback
    } else if interface.is_enable_dhcp_client() || interface.is_nat() {
        InterfaceTask::Uplink
    } else if radio.is_some() {
        InterfaceTask::RadioLink
    } else if interface.untagged_vlan().is_some() || interface.is_enable_dhcp_server() {
        // before the cables: a customer bridge may have other devices on its ports
        InterfaceTask::Customer
    } else if routeros_far.iter().any(|far| !far.is_bridge_device()) {
        InterfaceTask::Direct
    } else if routeros_far.iter().any(DeviceAccess::is_bridge_device) {
        InterfaceTask::RadioLink
    } else if addresses.iter().any(in_customer_prefix) {
        // after the cables: links of the stored export are in prefixes of no pool
        InterfaceTask::Customer
    } else {
        InterfaceTask::Unknown
    };
    let radio_path = (task == InterfaceTask::RadioLink).then(|| {
        if radio.is_some() {
            // on the bridge device
            let bridge_port = members
                .iter()
                .find(|member| member.is_ethernet_port())
                .map(|port| (*port).clone());
            let router_port = bridge_port
                .iter()
                .flat_map(InterfaceAccess::connected_interfaces)
                .find(|far| far.device().is_some_and(|d| d.has_routeros()));
            radio_path(router_port, bridge_port, radio.map(|radio| (*radio).clone()))
        } else {
            let bridge_port = members
                .iter()
                .flat_map(|member| member.connected_interfaces())
                .find(|far| far.device().is_some_and(|d| d.is_bridge_device()));
            let radio = bridge_port.as_ref().and_then(bridge_radio);
            radio_path(Some(interface.clone()), bridge_port, radio)
        }
    });
    let gateways = if task == InterfaceTask::Customer && addresses.is_empty() {
        interface
            .untagged_vlan()
            .map(|vlan| gateways(&vlan, interface))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let free = task == InterfaceTask::Unknown
        && ports.is_empty()
        && interface.cable().is_none()
        && addresses.is_empty()
        && interface.untagged_vlan().is_none()
        && interface.tagged_vlans().next().is_none()
        && interface.label().is_none()
        && !interface.use_ospf()
        && !interface.has_wireless_link()
        && !interface.is_bridge();
    let mut far_end_refs = far_ends.iter().map(interface_ref).collect::<Vec<_>>();
    far_end_refs.dedup();
    let row = InterfaceRow {
        task,
        interface: interface_ref(interface),
        label: interface.label().map(Into::into),
        ports: ports.iter().map(interface_ref).collect(),
        addresses: addresses.iter().map(address_ref).collect(),
        ospf: interface.use_ospf(),
        dhcp: interface.is_enable_dhcp_server(),
        dhcp_client: interface.is_enable_dhcp_client(),
        nat: interface.is_nat(),
        mtu: interface.mtu(),
        untagged_vlan: interface.untagged_vlan().as_ref().and_then(vlan_ref),
        tagged_vlans: interface.tagged_vlans().filter_map(|v| vlan_ref(&v)).collect(),
        far_ends: far_end_refs,
        radio_path,
        gateways,
        free,
        findings: Vec::new(),
        actions: Vec::new(),
    };
    let ids = members.iter().map(|member| member.id().0).collect();
    (row, ids)
}

/// What the helpers can do for `row`: a suggestion where something is missing, taking the link
/// or network out, a task for an unknown interface. No helper for uplinks.
fn actions(topology: &Arc<Topology>, row: &InterfaceRow, bridge_device: bool) -> Vec<RowAction> {
    let action = |kind, helper, interface: Option<u32>, peer: Option<InterfaceRef>, vlan| RowAction {
        kind,
        helper,
        interface,
        peer,
        vlan,
    };
    // the port (the row's interface or one of its bridge ports) cabled to a router, and that
    // router's port
    let link = std::iter::once(&row.interface)
        .chain(&row.ports)
        .filter_map(|member| topology.get_interface_by_id(InterfaceId(member.id)))
        .find_map(|member| {
            member
                .connected_interfaces()
                .into_iter()
                .find(|far| {
                    far.device()
                        .is_some_and(|d| d.has_routeros() && !d.is_bridge_device())
                })
                .map(|far| (member.id().0, interface_ref(&far)))
        });
    let own = Some(row.interface.id);
    let has_v4 = row.addresses.iter().any(|a| !a.address.contains(':'));
    let has_v6 = row.addresses.iter().any(|a| a.address.contains(':'));
    let customer_net = || {
        action(
            ActionKind::Suggestion,
            ActionHelper::CustomerNet,
            own.filter(|_| row.untagged_vlan.is_none()),
            None,
            row.untagged_vlan.as_ref().map(|vlan| vlan.id),
        )
    };
    let mut actions = Vec::new();
    match row.task {
        InterfaceTask::Loopback | InterfaceTask::Uplink => {}
        InterfaceTask::Direct => {
            if let Some((port, far)) = link {
                if !row.findings.is_empty() {
                    actions.push(action(
                        ActionKind::Suggestion,
                        ActionHelper::Connection,
                        Some(port),
                        Some(far),
                        None,
                    ));
                }
                actions.push(action(
                    ActionKind::Remove,
                    ActionHelper::Disconnect,
                    Some(port),
                    None,
                    None,
                ));
            }
        }
        InterfaceTask::RadioLink => {
            let incomplete = row.radio_path.as_ref().is_none_or(|path| {
                path.router_port.is_none()
                    || path.bridge_port.is_none()
                    || path.ssid.is_none()
                    || path.peer_bridge_port.is_none()
                    || path.peer_router_port.is_none()
            });
            // „Funkbrücke“ on the bridge device, or from the router port for the bridge device
            // cabled to it (`peer`)
            if incomplete || !row.findings.is_empty() {
                let bridge_port = row.radio_path.as_ref().and_then(|path| path.bridge_port.clone());
                if bridge_device {
                    actions.push(action(
                        ActionKind::Suggestion,
                        ActionHelper::RadioBridge,
                        None,
                        None,
                        None,
                    ));
                } else if let Some(bridge_port) = bridge_port {
                    actions.push(action(
                        ActionKind::Suggestion,
                        ActionHelper::RadioBridge,
                        row.radio_path.as_ref().and_then(|p| p.router_port.as_ref()).map(|p| p.id),
                        Some(bridge_port),
                        None,
                    ));
                }
            }
            if let Some(radio) = row.radio_path.as_ref().and_then(|p| p.radio.as_ref()) {
                actions.push(action(
                    ActionKind::Remove,
                    ActionHelper::RadioDisconnect,
                    Some(radio.id),
                    None,
                    None,
                ));
            }
        }
        InterfaceTask::Customer => {
            // with its L3 elsewhere a second gateway would be wrong
            // its VLAN on a port: the network reaches nobody yet
            if let Some(FindingKind::VlanWithoutPort(f)) = row
                .findings
                .iter()
                .map(|finding| &finding.kind)
                .find(|kind| matches!(kind, FindingKind::VlanWithoutPort(_)))
            {
                actions.push(action(
                    ActionKind::Suggestion,
                    ActionHelper::PortVlan,
                    None,
                    None,
                    Some(f.vlan.id),
                ));
            }
            if row.gateways.is_empty() && !(has_v4 && has_v6) {
                actions.push(customer_net());
            }
            actions.push(action(
                ActionKind::Remove,
                ActionHelper::CustomerDisconnect,
                own,
                None,
                None,
            ));
        }
        InterfaceTask::Unknown => {
            if !row.free {
                actions.push(RowAction {
                    kind: ActionKind::AssignRole,
                    ..customer_net()
                });
                actions.push(action(
                    ActionKind::AssignRole,
                    ActionHelper::Connection,
                    link.as_ref().map(|(port, _)| *port).or(own),
                    link.map(|(_, far)| far),
                    None,
                ));
                if bridge_device {
                    actions.push(action(
                        ActionKind::AssignRole,
                        ActionHelper::RadioBridge,
                        None,
                        None,
                        None,
                    ));
                }
                // a customer's VLAN on the port, its network elsewhere
                if topology
                    .get_interface_by_id(InterfaceId(row.interface.id))
                    .is_some_and(|i| i.is_ethernet_port())
                {
                    actions.push(action(
                        ActionKind::AssignRole,
                        ActionHelper::PortVlan,
                        own,
                        None,
                        None,
                    ));
                }
            }
        }
    }
    actions
}

/// An address in a prefix that is in no pool (`loopback`, `transfer`), itself or by a parent: a
/// customer network.
fn in_customer_prefix(address: &IpAddressAccess) -> bool {
    let mut prefix = address.prefix();
    let in_one = prefix.is_some();
    while let Some(current) = prefix {
        if current.data().is_some_and(|data| data.pool.is_some()) {
            return false;
        }
        prefix = current.parent();
    }
    in_one
}

/// The radio of the bridge device `port` is on: the one in the same bridge, else its only one.
fn bridge_radio(port: &InterfaceAccess) -> Option<InterfaceAccess> {
    let device = port.device()?;
    let radios = device
        .interfaces()
        .into_iter()
        .filter(InterfaceAccess::has_wireless_link)
        .collect::<Vec<_>>();
    if let Some(bridge) = port.bridge()
        && let Some(radio) = radios.iter().find(|r| r.bridge().as_ref() == Some(&bridge))
    {
        return Some(radio.clone());
    }
    match &radios[..] {
        [radio] => Some(radio.clone()),
        _ => None,
    }
}

/// The way from the router port over the bridge device's radio to the router on the other side.
fn radio_path(
    router_port: Option<InterfaceAccess>,
    bridge_port: Option<InterfaceAccess>,
    radio: Option<InterfaceAccess>,
) -> RadioPath {
    let peer_radio = radio.as_ref().and_then(InterfaceAccess::wireless_peer);
    let peer_bridge_port = peer_radio.as_ref().and_then(|peer| {
        let device = peer.device()?;
        let ports = device
            .interfaces()
            .into_iter()
            .filter(InterfaceAccess::is_ethernet_port)
            .collect::<Vec<_>>();
        ports
            .iter()
            .find(|port| port.bridge().is_some() && port.bridge() == peer.bridge())
            .or_else(|| ports.iter().find(|port| port.cable().is_some()))
            .cloned()
    });
    let peer_router_port = peer_bridge_port.as_ref().and_then(|port| {
        port.connected_interfaces()
            .into_iter()
            .find(|far| far.device().is_some_and(|d| d.has_routeros()))
    });
    RadioPath {
        router_port: router_port.as_ref().map(interface_ref),
        bridge_port: bridge_port.as_ref().map(interface_ref),
        ssid: radio
            .as_ref()
            .and_then(|radio| radio.wireless_link())
            .map(|link| link.link.ssid.clone())
            .filter(|ssid| !ssid.is_empty()),
        radio: radio.as_ref().map(interface_ref),
        peer_radio: peer_radio.as_ref().map(interface_ref),
        peer_bridge_port: peer_bridge_port.as_ref().map(interface_ref),
        peer_router_port: peer_router_port.as_ref().map(interface_ref),
    }
}

/// The RouterOS devices other than the one of `interface` with an address on `vlan`: on an
/// interface with it untagged, or on that interface's bridge.
/// The other devices with an address in `vlan` where `interface` is in it (its domain,
/// `vlan_domain`).
fn gateways(vlan: &VlanAccess, interface: &InterfaceAccess) -> Vec<DeviceRef> {
    let Some(own) = interface.device() else {
        return Vec::new();
    };
    let topology = vlan.topology();
    let mut devices = vlan_domain::domain_of(vlan, &own)
        .into_iter()
        .filter(|id| *id != own.id())
        .filter_map(|id| topology.get_device_by_id(&id))
        .filter(|device| !vlan_domain::l3_interfaces(vlan, device).is_empty())
        .map(|device| device_ref(&device))
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

impl FindingKind {
    /// About the device as a whole, not one of its interfaces.
    fn is_about_device(&self) -> bool {
        matches!(
            self,
            FindingKind::NoLoopback(_)
                | FindingKind::NoPrimaryIpv4(_)
                | FindingKind::PrimaryIpv4NotOnLoopback(_)
                | FindingKind::LoopbackWithoutOspf(_)
        )
    }
    /// The NetBox ids of the interfaces it names.
    fn interfaces(&self) -> Vec<u32> {
        match self {
            FindingKind::PoolMissing(_)
            | FindingKind::PoolNearlyFull(_)
            | FindingKind::NoLoopback(_)
            | FindingKind::NoPrimaryIpv4(_)
            | FindingKind::LoopbackWithoutOspf(_)
            | FindingKind::WlanWithoutVlan(_) => Vec::new(),
            FindingKind::PrimaryIpv4NotOnLoopback(f) => {
                f.interface.iter().map(|i| i.id).collect()
            }
            FindingKind::AddressWithoutPrefix(f) => vec![f.interface.id],
            FindingKind::DhcpPoolEmpty(f) => vec![f.interface.id],
            FindingKind::PortLikeName(f) => vec![f.interface.id],
            FindingKind::RadioWithoutRole(f) => vec![f.interface.id],
            FindingKind::VlanWithoutGroup(f) => vec![f.interface.id],
            FindingKind::MtuMismatch(f) => vec![f.interface.id, f.far_end.id],
            FindingKind::OspfOnOneEnd(f) => vec![f.interface.id, f.far_end.id],
            FindingKind::NoCommonSubnet(f) => vec![f.interface.id, f.far_end.id],
            FindingKind::AddressWithoutOspf(f) => vec![f.interface.id, f.far_end.id],
            FindingKind::VlanWithSeveralGateways(f) => f.interfaces.iter().map(|i| i.id).collect(),
            FindingKind::VlanWithoutGateway(f) => f.interfaces.iter().map(|i| i.id).collect(),
            FindingKind::TaggedVlanOnPort(f) => vec![f.interface.id],
            FindingKind::RadioPathIncomplete(f) => vec![f.radio.id],
            FindingKind::LabelNotFarEnd(f) => vec![f.interface.id],
            FindingKind::VlanWithoutPort(f) => vec![f.interface.id],
        }
    }
}

#[cfg(test)]
pub(crate) mod test;
