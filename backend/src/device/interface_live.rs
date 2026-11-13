//! Scope: The live checks of the interface view: what the device sees and has at each row; not the
//! rows from NetBox (`topology::interfaces`).
//!
//! The live checks of the interface view (`docs/interface-view.md`): who the device
//! sees at each row's ports (`/ip/neighbor`, as „Nachbarn lesen“), its OSPF neighbors there, and
//! whether the bridge devices of a radio link answer a ping at their loopback, and where the
//! device contradicts NetBox's PoE modes (`device::poe::mismatches`). Read only; the
//! rows are those of `topology::interfaces`, so the frontend joins both by the row's interface.
//!
//! An OSPF neighbor names the interface as RouterOS calls it: a port by its short name and label
//! (`e01-sxt-a`), a bridge or VLAN interface by what L2 generated (`bridge-link` for NetBox'
//! `link`). So beyond the port names it goes by the addresses `/ip/address` has there.
use crate::topology::interfaces::RowOf;
use crate::{
    Error,
    device::poe::{self, PoeMismatch, PoeProblem},
    device::{
        access::answering,
        neighbors::{self, NeighborFinding, NeighborFindingKind, SeenNeighbor, port_named},
        ros::dhcp::static_leases,
        update_fallback::is_adjacent,
    },
    topology::{
        DeviceId, InterfaceId,
        access::device::DeviceAccess,
        findings::{DeviceRef, device_ref},
        interfaces::{
            ActionHelper, ActionKind, InterfaceRow, InterfaceTask, LeaseSeen, RowAction, SeenDevice,
        },
    },
};
use async_graphql::SimpleObject;
use ipnet::IpNet;
use mikrotik_model::{
    command::Reads,
    model::{
        InterfaceState, IpAddress, IpDhcpServer, IpDhcpServerLease, Ipv6Address,
        RoutingOspfNeighborState,
    },
    resource::collect_resource,
    value::{HasNone, RosValue},
};
use std::{collections::BTreeSet, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceLive {
    pub rows: Box<[LiveRow]>,
    /// the neighbor findings on no row (a port NetBox doesn't have, discovery off)
    pub other_findings: Box<[NeighborFinding]>,
    /// the addresses only the device has on an interface of no row
    pub other_addresses: Box<[DeviceOnlyAddress]>,
    /// the static leases only the device has of a server on no row
    pub other_leases: Box<[DeviceOnlyLease]>,
}

/// What the device shows at a row.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LiveRow {
    /// the row's interface (`InterfaceRow.interface`)
    pub interface: u32,
    /// who the device sees at its ports
    pub seen: Vec<SeenNeighbor>,
    /// its OSPF neighbors at the row's interface
    pub ospf: Box<[OspfNeighbor]>,
    /// the routers NetBox puts at the other end (OSPF on the row) with no adjacency there
    pub ospf_missing: Box<[DeviceRef]>,
    /// an OSPF neighbor is adjacent at the row, whose interface NetBox has without `ospf`: the
    /// next apply takes OSPF off there and cuts it (E8)
    pub ospf_untagged: bool,
    /// the bridge devices of a radio link, asked at their primary IPv4
    pub bridges: Box<[BridgeAnswer]>,
    /// the neighbor findings about its ports, the worst first
    pub findings: Vec<NeighborFinding>,
    /// the addresses the device has at the row's interfaces and NetBox doesn't: the next apply
    /// removes them
    pub device_only_addresses: Box<[DeviceOnlyAddress]>,
    /// the static DHCP leases of the row's server that NetBox has no client for: the next
    /// apply removes them
    pub device_only_leases: Box<[DeviceOnlyLease]>,
    /// the row's ports whose PoE mode in NetBox the device contradicts
    pub poe: Vec<PoeMismatch>,
    /// what the findings suggest: the link to the router the device sees at a port, an
    /// address to take over
    pub actions: Vec<RowAction>,
}

/// A static lease of `/ip/dhcp-server/lease` NetBox has no client for (`device::ros::dhcp`:
/// a device without RouterOS whose interface has a MAC and the address).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceOnlyLease {
    pub address: Box<str>,
    /// none for a lease by client ID, which NetBox can't hold
    pub mac: Option<Box<str>>,
    pub client_id: Option<Box<str>>,
    /// its server; none for one of all servers
    pub server: Option<Box<str>>,
    /// the host name its client sent, else its comment
    pub name: Option<Box<str>>,
    pub disabled: bool,
    /// its settings NetBox doesn't hold and a takeover loses (`block-access`, `rate-limit` …)
    pub settings: Box<[Box<str>]>,
    /// NetBox has the address on this device, which isn't a lease client (RouterOS, no MAC)
    pub elsewhere: Option<DeviceRef>,
    /// NetBox has the address on no interface
    pub unassigned: bool,
}

/// A static address of `/ip/address` or `/ipv6/address` (no link-local one) that NetBox
/// doesn't have on the device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct DeviceOnlyAddress {
    /// with its length, `192.168.1.1/24`
    pub address: Box<str>,
    /// the interface as RouterOS names it
    pub interface_name: Box<str>,
    /// its NetBox interface, by that name or another address there; where it is taken over
    pub interface: Option<u32>,
    pub disabled: bool,
    /// the device NetBox has the address on instead; it isn't offered for taking over then
    pub elsewhere: Option<DeviceRef>,
    /// NetBox has the address on no interface: taking it over is for NetBox' own hands
    pub unassigned: bool,
}

/// A neighbor of `/routing/ospf/neighbor`.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct OspfNeighbor {
    pub instance: Box<str>,
    pub address: Box<str>,
    pub router_id: Box<str>,
    pub state: Box<str>,
    /// `Full`, or `TwoWay` between two DROthers: where OSPF leaves it
    pub adjacent: bool,
    /// the NetBox device whose primary IPv4 is its router ID (or which has its address)
    pub device: Option<DeviceRef>,
}

/// A bridge device of a radio link and whether it answered three pings from the device.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct BridgeAnswer {
    pub device: DeviceRef,
    /// its primary IPv4; without one it isn't asked
    pub address: Option<Box<str>>,
    pub answers: bool,
}

/// Reads the device and checks its rows.
pub async fn read(client: &impl Reads, device: &DeviceAccess) -> Result<InterfaceLive, Error> {
    let neighbors = neighbors::read(client, device).await?;
    let ospf = collect_resource::<RoutingOspfNeighborState>(client).await?;
    let mut addresses = collect_resource::<IpAddress>(client)
        .await?
        .iter()
        .map(DeviceAddress::from)
        .collect::<Vec<_>>();
    addresses.extend(
        collect_resource::<Ipv6Address>(client)
            .await?
            .iter()
            .map(DeviceAddress::from),
    );
    // an apply keeps the addresses on interfaces set up by hand: nothing to show of them
    let interfaces = collect_resource::<InterfaceState>(client)
        .await?
        .iter()
        .map(|interface| TypedInterface {
            name: interface.name.to_string().into(),
            kind: interface._type.to_string().into(),
        })
        .collect::<Vec<_>>();
    let hand_made = hand_made_interfaces(&interfaces);
    addresses.retain(|address| !hand_made.contains(&address.interface));
    let dhcp = Dhcp {
        servers: collect_resource::<IpDhcpServer>(client)
            .await?
            .iter()
            .map(|server| DhcpServer {
                name: server.cfg.name.to_string().into(),
                interface: server.cfg.interface.to_string().into(),
            })
            .collect(),
        leases: collect_resource::<IpDhcpServerLease>(client)
            .await?
            .iter()
            .map(DeviceLease::from)
            .collect(),
    };
    let rows = device.interface_rows();
    let bridges = rows
        .iter()
        .flat_map(|of| bridge_devices(device, &of.row))
        .collect::<BTreeSet<_>>();
    let loopbacks = bridges
        .iter()
        .filter_map(|id| {
            device
                .topology()
                .get_device_by_id(&DeviceId(*id))?
                .primary_ip_v4()?
                .addr()
        })
        .collect::<Vec<_>>();
    let answering = answering(client, &loopbacks).await;
    let poe = poe::mismatches(device, &poe::read_ports(client).await?);
    let mut live = check(
        device, &rows, neighbors, &ospf, &addresses, &dhcp, &answering,
    );
    place_poe(&mut live, &rows, poe);
    Ok(live)
}

/// Puts each PoE mismatch on the row of its port, with „PoE-Eintrag entfernen“, or „PoE
/// vervollständigen“ (`PdMissing`: „PoE“ from the port, the far end from its cable).
pub fn place_poe(live: &mut InterfaceLive, rows: &[RowOf], poe: Vec<PoeMismatch>) {
    for mismatch in poe {
        let Some(row) = rows
            .iter()
            .position(|of| of.members.contains(&mismatch.port.id))
            .and_then(|index| live.rows.get_mut(index))
        else {
            continue;
        };
        row.actions.push(RowAction {
            kind: ActionKind::Suggestion,
            helper: match mismatch.problem {
                PoeProblem::PdMissing => ActionHelper::Poe,
                _ => ActionHelper::PoeRemove,
            },
            interface: Some(mismatch.port.id),
            peer: None,
            vlan: None,
            seen: None,
            address: None,
            lease: None,
        });
        row.poe.push(mismatch);
    }
}

/// What the device answers about DHCP: its servers and leases.
#[derive(Debug, Default)]
pub struct Dhcp {
    pub servers: Box<[DhcpServer]>,
    pub leases: Box<[DeviceLease]>,
}

/// A DHCP server of the device (`/ip/dhcp-server`).
#[derive(Debug)]
pub struct DhcpServer {
    pub name: Box<str>,
    pub interface: Box<str>,
}

/// An address of the device (`/ip/address`, `/ipv6/address`).
#[derive(Debug)]
pub struct DeviceAddress {
    /// the interface as RouterOS names it
    pub interface: Box<str>,
    pub address: IpNet,
    pub dynamic: bool,
    pub disabled: bool,
}

impl From<&IpAddress> for DeviceAddress {
    fn from(address: &IpAddress) -> Self {
        DeviceAddress {
            interface: address.cfg.interface.to_string().into(),
            address: IpNet::V4(address.cfg.address),
            dynamic: address.status.dynamic,
            disabled: address.status.disabled,
        }
    }
}

impl From<&Ipv6Address> for DeviceAddress {
    fn from(address: &Ipv6Address) -> Self {
        DeviceAddress {
            interface: address.cfg.interface.to_string().into(),
            address: IpNet::V6(address.cfg.address),
            dynamic: address.status.dynamic,
            disabled: address.cfg.disabled,
        }
    }
}

/// A lease of the device's DHCP servers (`/ip/dhcp-server/lease`).
#[derive(Debug)]
pub struct DeviceLease {
    pub address: IpAddr,
    pub mac: Option<Box<str>>,
    pub client_id: Option<Box<str>>,
    /// none: of all servers
    pub server: Option<Box<str>>,
    pub host_name: Option<Box<str>>,
    pub comment: Option<Box<str>>,
    pub disabled: bool,
    pub dynamic: bool,
    /// the settings it has that NetBox doesn't hold, by their RouterOS names
    pub settings: Box<[Box<str>]>,
}

impl From<&IpDhcpServerLease> for DeviceLease {
    fn from(lease: &IpDhcpServerLease) -> Self {
        let cfg = &lease.cfg;
        let text = |value: Option<&mikrotik_model::ascii::AsciiString>| {
            value
                .map(ToString::to_string)
                .filter(|value| !value.is_empty())
                .map(Box::from)
        };
        let settings = [
            ("address-lists", !cfg.address_lists.is_empty()),
            ("allow-dual-stack-queue", cfg.allow_dual_stack_queue),
            ("always-broadcast", cfg.always_broadcast),
            ("block-access", cfg.block_access),
            ("dhcp-option", text(cfg.dhcp_option.as_ref()).is_some()),
            (
                "dhcp-option-set",
                matches!(&cfg.dhcp_option_set, Some(HasNone::Value(set)) if !set.0.is_empty()),
            ),
            (
                "insert-queue-before",
                text(cfg.insert_queue_before.as_ref()).is_some(),
            ),
            ("lease-time", !cfg.lease_time.is_zero()),
            ("queue-type", text(cfg.queue_type.as_ref()).is_some()),
            ("rate-limit", text(cfg.rate_limit.as_ref()).is_some()),
            ("routes", text(cfg.routes.as_ref()).is_some()),
            ("use-src-mac", cfg.use_src_mac),
        ];
        DeviceLease {
            address: cfg.address,
            mac: cfg.mac_address.map(|mac| mac.to_string().into()),
            client_id: text(cfg.client_id.as_ref()),
            server: text(cfg.server.as_ref()),
            host_name: lease
                .status
                .host_name
                .as_ref()
                .map(|name| String::from_utf8_lossy(&name.encode_ros()).into()),
            comment: text(cfg.comment.as_ref()),
            disabled: cfg.disabled,
            dynamic: lease.status.dynamic,
            settings: settings
                .into_iter()
                .filter(|(_, set)| *set)
                .map(|(name, _)| Box::from(name))
                .collect(),
        }
    }
}

/// An interface of the device and its RouterOS type (`/interface`).
pub struct TypedInterface {
    pub name: Box<str>,
    pub kind: Box<str>,
}

/// [`read`] on what the device answered.
pub fn check(
    device: &DeviceAccess,
    rows: &[RowOf],
    neighbors: neighbors::Neighbors,
    ospf: &[RoutingOspfNeighborState],
    addresses: &[DeviceAddress],
    dhcp: &Dhcp,
    answering: &BTreeSet<IpAddr>,
) -> InterfaceLive {
    let topology = device.topology();
    // the NetBox interface of a RouterOS interface name: the port of that name, else the one
    // with an address RouterOS has on it
    let netbox_interface = |name: &str| {
        port_named(device, name).map(|i| i.id().0).or_else(|| {
            let on_it = addresses
                .iter()
                .filter(|address| *address.interface == *name)
                .map(|address| address.address.addr())
                .collect::<Vec<_>>();
            device
                .interfaces()
                .iter()
                .find(|i| {
                    i.ips()
                        .iter()
                        .any(|ip| ip.addr().is_some_and(|ip| on_it.contains(&ip)))
                })
                .map(|i| i.id().0)
        })
    };
    let ospf_device = |neighbor: &RoutingOspfNeighborState| {
        let router_id = IpAddr::V4(neighbor.router_id);
        topology
            .list_devices()
            .find(|d| d.primary_ip_v4().and_then(|ip| ip.addr()) == Some(router_id))
            .or_else(|| {
                topology.list_devices().find(|d| {
                    d.interfaces()
                        .iter()
                        .flat_map(|i| i.ips())
                        .any(|ip| ip.addr() == Some(neighbor.address))
                })
            })
    };
    let device_only = device_only_addresses(device, addresses, netbox_interface);
    let mut taken = vec![false; device_only.len()];
    let leases = device_only_leases(device, dhcp);
    let mut leases_taken = vec![false; leases.len()];
    // a lease's row: its server's interface, else the interface with its network
    let lease_interface = |lease: &DeviceOnlyLease| {
        lease
            .server
            .as_deref()
            .and_then(|server| dhcp.servers.iter().find(|known| *known.name == *server))
            .and_then(|server| netbox_interface(&server.interface))
            .or_else(|| {
                let address = lease.address.parse::<IpAddr>().ok()?;
                device
                    .interfaces()
                    .iter()
                    .find(|i| {
                        i.ips()
                            .iter()
                            .any(|ip| ip.net().is_some_and(|net| net.contains(&address)))
                    })
                    .map(|i| i.id().0)
            })
    };
    let mut placed = vec![false; neighbors.findings.len()];
    let live_rows = rows
        .iter()
        .map(|RowOf { row, members }| {
            let seen = members
                .iter()
                .flat_map(|id| neighbors.seen.get(id).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            let ospf_here = ospf
                .iter()
                .filter(|neighbor| {
                    neighbor
                        .interface
                        .as_ref()
                        .and_then(|interface| netbox_interface(&interface.to_string()))
                        .is_some_and(|interface| members.contains(&interface))
                })
                .map(|neighbor| OspfNeighbor {
                    instance: neighbor.instance.to_string().into(),
                    address: neighbor.address.to_string().into(),
                    router_id: neighbor.router_id.to_string().into(),
                    state: String::from_utf8_lossy(&neighbor.state.encode_ros()).into(),
                    adjacent: is_adjacent(ospf, neighbor),
                    device: ospf_device(neighbor).as_ref().map(device_ref),
                })
                .collect::<Vec<_>>();
            let ospf_missing = expected_ospf(device, row)
                .into_iter()
                .filter(|expected| {
                    !ospf_here.iter().any(|n| {
                        n.adjacent && n.device.as_ref().is_some_and(|d| d.id == expected.id)
                    })
                })
                .collect();
            let ospf_untagged = !row.ospf && ospf_here.iter().any(|n| n.adjacent);
            let bridges = bridge_devices(device, row)
                .into_iter()
                .filter_map(|id| topology.get_device_by_id(&DeviceId(id)))
                .map(|bridge| {
                    let address = bridge.primary_ip_v4().and_then(|ip| ip.addr());
                    BridgeAnswer {
                        device: device_ref(&bridge),
                        address: address.map(|a| a.to_string().into()),
                        answers: address.is_some_and(|a| answering.contains(&a)),
                    }
                })
                .collect();
            let findings: Vec<NeighborFinding> = neighbors
                .findings
                .iter()
                .zip(placed.iter_mut())
                .filter(|(finding, _)| port_of(finding).is_some_and(|id| members.contains(&id)))
                .map(|(finding, placed)| {
                    *placed = true;
                    finding.clone()
                })
                .collect();
            let mut actions = Vec::<RowAction>::new();
            let suggestions = findings.iter().filter_map(|finding| {
                seen_link(device, finding).or_else(|| new_device(device, finding))
            });
            for action in suggestions {
                if !actions.contains(&action) {
                    actions.push(action);
                }
            }
            let device_only_addresses = device_only
                .iter()
                .zip(taken.iter_mut())
                .filter(|(address, _)| address.interface.is_some_and(|id| members.contains(&id)))
                .map(|(address, taken)| {
                    *taken = true;
                    address.clone()
                })
                .collect::<Vec<_>>();
            actions.extend(device_only_addresses.iter().filter_map(address_takeover));
            let device_only_leases = leases
                .iter()
                .zip(leases_taken.iter_mut())
                .filter(|(lease, _)| lease_interface(lease).is_some_and(|id| members.contains(&id)))
                .map(|(lease, taken)| {
                    *taken = true;
                    lease.clone()
                })
                .collect::<Vec<_>>();
            actions.extend(
                device_only_leases
                    .iter()
                    .filter_map(|lease| lease_takeover(lease, row.interface.id)),
            );
            LiveRow {
                interface: row.interface.id,
                seen,
                ospf: ospf_here.into(),
                ospf_missing,
                ospf_untagged,
                bridges,
                findings,
                device_only_addresses: device_only_addresses.into(),
                device_only_leases: device_only_leases.into(),
                poe: Vec::new(),
                actions,
            }
        })
        .collect();
    let other_findings = neighbors
        .findings
        .into_iter()
        .zip(placed)
        .filter(|(_, placed)| !placed)
        .map(|(finding, _)| finding)
        .collect();
    let other_addresses = device_only
        .into_iter()
        .zip(taken)
        .filter(|(_, taken)| !taken)
        .map(|(address, _)| address)
        .collect();
    let other_leases = leases
        .into_iter()
        .zip(leases_taken)
        .filter(|(_, taken)| !taken)
        .map(|(lease, _)| lease)
        .collect();
    InterfaceLive {
        rows: live_rows,
        other_findings,
        other_addresses,
        other_leases,
    }
}

/// The host name a DHCP client sent, up to its first control character: RouterOS reports it as
/// sent, and a client padding a fixed field sends NUL bytes after it (a Tinkerforge brick's
/// `Tinkerforge5093BA` came with a row of them).
fn client_name(name: &str) -> Option<Box<str>> {
    let name = name
        .split(|c: char| c.is_control() || c == char::REPLACEMENT_CHARACTER)
        .next()
        .unwrap_or_default()
        .trim();
    (!name.is_empty()).then(|| name.into())
}

/// The static leases of the device (`/ip/dhcp-server/lease` rows) its NetBox target doesn't
/// have: the addresses of no lease client in its DHCP networks (`device::ros::dhcp`).
fn device_only_leases(device: &DeviceAccess, dhcp: &Dhcp) -> Vec<DeviceOnlyLease> {
    let topology = device.topology();
    let expected = device
        .interfaces()
        .iter()
        .filter(|i| i.is_enable_dhcp_server())
        .flat_map(|i| i.ips())
        .filter_map(|ip| ip.prefix())
        .flat_map(|prefix| static_leases(&prefix))
        .map(|lease| IpAddr::V4(lease.address))
        .collect::<BTreeSet<_>>();
    dhcp.leases
        .iter()
        .filter(|lease| !lease.dynamic)
        .filter_map(|lease| {
            let address = lease.address;
            if expected.contains(&address) {
                return None;
            }
            let netbox = topology.ip_address_with_host(address);
            Some(DeviceOnlyLease {
                address: address.to_string().into(),
                mac: lease.mac.clone(),
                client_id: lease.client_id.clone(),
                server: lease.server.clone(),
                name: lease
                    .host_name
                    .as_deref()
                    .and_then(client_name)
                    .or_else(|| lease.comment.clone()),
                disabled: lease.disabled,
                settings: lease.settings.clone(),
                elsewhere: netbox
                    .and_then(|ip| topology.get_interface_by_id(ip.interface?)?.device())
                    .as_ref()
                    .map(device_ref),
                unassigned: netbox.is_some_and(|ip| ip.interface.is_none()),
            })
        })
        .collect()
}

/// „Lease übernehmen“ for a lease by MAC whose address NetBox has nowhere, from the row of its
/// server (`interface`).
fn lease_takeover(lease: &DeviceOnlyLease, interface: u32) -> Option<RowAction> {
    if lease.elsewhere.is_some() || lease.unassigned {
        return None;
    }
    Some(RowAction {
        kind: ActionKind::Suggestion,
        helper: ActionHelper::LeaseTakeover,
        interface: Some(interface),
        peer: None,
        vlan: None,
        seen: None,
        address: Some(lease.address.clone()),
        lease: Some(LeaseSeen {
            mac: lease.mac.clone()?,
            name: lease.name.clone(),
        }),
    })
}

/// The RouterOS types of the interfaces the provisioner manages (`BaseDeviceDataTarget::
/// keep_hand_made_addresses` goes by their names): ports, bridges, VLANs, VXLANs, radios, `lo`.
const MANAGED_TYPES: [&str; 8] = [
    "ether", "bridge", "vlan", "vxlan", "wlan", "wifi", "w60g", "loopback",
];

/// The names of the interfaces of another type (`/interface` rows): tunnels, PPPoE clients and
/// whatever else is set up by hand, whose addresses an apply keeps.
fn hand_made_interfaces(interfaces: &[TypedInterface]) -> BTreeSet<Box<str>> {
    interfaces
        .iter()
        .filter(|interface| !MANAGED_TYPES.contains(&&*interface.kind))
        .map(|interface| interface.name.clone())
        .collect()
}

/// The static addresses of the device (`/ip/address` and `/ipv6/address` rows) that NetBox
/// doesn't have on it, link-local ones left out (RouterOS makes them).
fn device_only_addresses(
    device: &DeviceAccess,
    addresses: &[DeviceAddress],
    netbox_interface: impl Fn(&str) -> Option<u32>,
) -> Vec<DeviceOnlyAddress> {
    let topology = device.topology();
    let own = device
        .interfaces()
        .iter()
        .flat_map(|i| i.ips())
        .filter_map(|ip| ip.addr())
        .chain(device.loopback_ip().and_then(|ip| ip.addr()))
        .collect::<BTreeSet<_>>();
    addresses
        .iter()
        .filter(|row| !row.dynamic)
        .filter_map(|row| {
            let address = row.address;
            let link_local = match address.addr() {
                IpAddr::V6(v6) => v6.is_unicast_link_local(),
                IpAddr::V4(_) => false,
            };
            if link_local || own.contains(&address.addr()) {
                return None;
            }
            let interface_name = &*row.interface;
            let netbox = topology.ip_address_with_host(address.addr());
            Some(DeviceOnlyAddress {
                address: address.to_string().into(),
                interface_name: interface_name.into(),
                interface: netbox_interface(interface_name),
                disabled: row.disabled,
                elsewhere: netbox
                    .and_then(|ip| topology.get_interface_by_id(ip.interface?)?.device())
                    .as_ref()
                    .map(device_ref),
                unassigned: netbox.is_some_and(|ip| ip.interface.is_none()),
            })
        })
        .collect()
}

/// „Adresse übernehmen“ for an address NetBox has nowhere, at its NetBox interface.
fn address_takeover(address: &DeviceOnlyAddress) -> Option<RowAction> {
    if address.elsewhere.is_some() || address.unassigned {
        return None;
    }
    Some(RowAction {
        kind: ActionKind::Suggestion,
        helper: ActionHelper::AddressTakeover,
        interface: Some(address.interface?),
        peer: None,
        vlan: None,
        seen: None,
        address: Some(address.address.clone()),
        lease: None,
    })
}

/// The NetBox interface a neighbor finding is about.
fn port_of(finding: &NeighborFinding) -> Option<u32> {
    let port = match &finding.kind {
        NeighborFindingKind::CableMissing(f) => &f.port,
        NeighborFindingKind::CableElsewhere(f) => &f.port,
        NeighborFindingKind::OtherFarPort(f) => &f.port,
        NeighborFindingKind::NeighborNotSeen(f) => &f.port,
        NeighborFindingKind::NeighborsNotInNetbox(f) => &f.port,
        NeighborFindingKind::DiscoveryOff(_) => return None,
    };
    port.interface.as_ref().map(|interface| interface.id)
}

/// The link to the one router the device sees at a port where NetBox has no cable
/// („Verbindung“) or one elsewhere („Kabel übernehmen“). Radio bridge devices are left out:
/// their links are radio links.
fn seen_link(device: &DeviceAccess, finding: &NeighborFinding) -> Option<RowAction> {
    let (port, seen, helper) = match &finding.kind {
        NeighborFindingKind::CableMissing(f) => {
            (&f.port, f.seen.as_slice(), ActionHelper::Connection)
        }
        NeighborFindingKind::CableElsewhere(f) => {
            (&f.port, f.seen.as_slice(), ActionHelper::CableTakeover)
        }
        NeighborFindingKind::OtherFarPort(f) => (
            &f.port,
            std::slice::from_ref(&f.seen),
            ActionHelper::CableTakeover,
        ),
        _ => return None,
    };
    let [seen] = seen else { return None };
    let (port, peer) = (port.interface.as_ref()?, seen.interface.as_ref()?);
    let topology = device.topology();
    let peer_device = topology.get_device_by_id(&DeviceId(peer.device.id))?;
    let routers = [device, &peer_device];
    if routers
        .iter()
        .any(|d| !d.has_routeros() || d.is_bridge_device())
    {
        return None;
    }
    Some(RowAction {
        kind: ActionKind::Suggestion,
        helper,
        interface: Some(port.id),
        peer: Some(peer.clone()),
        vlan: None,
        seen: None,
        address: None,
        lease: None,
    })
}

/// A new device for the one MikroTik (it reports a board) the device sees at a free port (no
/// cable, or one ending open at a patch panel) that NetBox doesn't know („Neues Gerät“), on a
/// router: a bridge device's ports are its radio link.
fn new_device(device: &DeviceAccess, finding: &NeighborFinding) -> Option<RowAction> {
    let NeighborFindingKind::NeighborsNotInNetbox(f) = &finding.kind else {
        return None;
    };
    let [seen] = f.seen.as_slice() else {
        return None;
    };
    let port = f.port.interface.as_ref()?;
    let free = device
        .topology()
        .get_interface_by_id(InterfaceId(port.id))
        .is_some_and(|i| i.is_ethernet_port() && crate::helper::new_device::landing(&i).is_some());
    if !free || seen.board.is_empty() || !device.has_routeros() || device.is_bridge_device() {
        return None;
    }
    Some(RowAction {
        kind: ActionKind::Suggestion,
        helper: ActionHelper::NewDevice,
        interface: Some(port.id),
        peer: None,
        vlan: None,
        seen: Some(SeenDevice {
            identity: seen.identity.clone(),
            board: seen.board.clone(),
            interface_name: seen.interface_name.clone(),
        }),
        address: None,
        lease: None,
    })
}

/// The routers an `ospf` row should be adjacent with: the RouterOS devices at its cables, and on
/// a radio link every device of the path with `ospf` somewhere (the bridge devices are OSPF
/// routers through their bridge).
fn expected_ospf(device: &DeviceAccess, row: &InterfaceRow) -> Vec<DeviceRef> {
    if !row.ospf {
        return Vec::new();
    }
    let topology = device.topology();
    let ids = match (row.task, &row.radio_path) {
        (InterfaceTask::Direct, _) => row.far_ends.iter().map(|far| far.device.id).collect(),
        (InterfaceTask::RadioLink, Some(path)) => [
            &path.router_port,
            &path.bridge_port,
            &path.peer_bridge_port,
            &path.peer_router_port,
        ]
        .into_iter()
        .flatten()
        .map(|end| end.device.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect(),
        _ => Vec::new(),
    };
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| *id != device.id().0 && seen.insert(*id))
        .filter_map(|id| topology.get_device_by_id(&DeviceId(id)))
        .filter(|d| d.has_routeros() && d.interfaces().iter().any(|i| i.use_ospf()))
        .map(|d| device_ref(&d))
        .collect()
}

/// The bridge devices of a radio link row other than the device itself.
fn bridge_devices(device: &DeviceAccess, row: &InterfaceRow) -> Vec<u32> {
    let Some(path) = &row.radio_path else {
        return Vec::new();
    };
    [&path.bridge_port, &path.peer_bridge_port]
        .into_iter()
        .flatten()
        .map(|end| end.device.id)
        .filter(|id| *id != device.id().0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod test;
