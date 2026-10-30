//! The live checks of the interface view (`docs/interface-view.md`): who the device
//! sees at each row's ports (`/ip/neighbor`, as „Nachbarn lesen“), its OSPF neighbors there, and
//! whether the bridge devices of a radio link answer a ping at their loopback, and where the
//! device contradicts NetBox's PoE modes (`helper::poe::mismatches`). Read only; the
//! rows are those of `topology::interfaces`, so the frontend joins both by the row's interface.
//!
//! An OSPF neighbor names the interface as RouterOS calls it: a port by its short name and label
//! (`e01-sxt-a`), a bridge or VLAN interface by what L2 generated (`bridge-link` for NetBox'
//! `link`). So beyond the port names it goes by the addresses `/ip/address` has there.
use crate::{
    Error,
    device::{
        neighbors::{self, NeighborFinding, NeighborFindingKind, SeenNeighbor, port_named},
        raw::{Row, answering, field, rows},
        ros::dhcp::static_leases,
        update_fallback::is_adjacent,
    },
    helper::poe::{self, PoeMismatch, PoeProblem},
    topology::{
        DeviceId, InterfaceId,
        access::device::DeviceAccess,
        findings::{DeviceRef, device_ref},
        interfaces::{
            ActionHelper, ActionKind, InterfaceRow, InterfaceTask, LeaseSeen, RowAction,
            SeenDevice,
        },
    },
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use ipnet::IpNet;
use std::{collections::BTreeSet, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct InterfaceLive {
    pub rows: Vec<LiveRow>,
    /// the neighbor findings on no row (a port NetBox doesn't have, discovery off)
    pub other_findings: Vec<NeighborFinding>,
    /// the addresses only the device has on an interface of no row
    pub other_addresses: Vec<DeviceOnlyAddress>,
    /// the static leases only the device has of a server on no row
    pub other_leases: Vec<DeviceOnlyLease>,
}

/// What the device shows at a row.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct LiveRow {
    /// the row's interface (`InterfaceRow.interface`)
    pub interface: u32,
    /// who the device sees at its ports
    pub seen: Vec<SeenNeighbor>,
    /// its OSPF neighbors at the row's interface
    pub ospf: Vec<OspfNeighbor>,
    /// the routers NetBox puts at the other end (OSPF on the row) with no adjacency there
    pub ospf_missing: Vec<DeviceRef>,
    /// the bridge devices of a radio link, asked at their primary IPv4
    pub bridges: Vec<BridgeAnswer>,
    /// the neighbor findings about its ports, the worst first
    pub findings: Vec<NeighborFinding>,
    /// the addresses the device has at the row's interfaces and NetBox doesn't: the next apply
    /// removes them
    pub device_only_addresses: Vec<DeviceOnlyAddress>,
    /// the static DHCP leases of the row's server that NetBox has no client for: the next
    /// apply removes them
    pub device_only_leases: Vec<DeviceOnlyLease>,
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
    pub settings: Vec<Box<str>>,
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
pub async fn read(client: &RawDevice<SimpleResult>, device: &DeviceAccess) -> Result<InterfaceLive, Error> {
    let neighbors = neighbors::read(client, device).await?;
    let ospf = rows(client, "/routing/ospf/neighbor/print").await?;
    let mut addresses = rows(client, "/ip/address/print").await?;
    addresses.extend(rows(client, "/ipv6/address/print").await?);
    // an apply keeps the addresses on interfaces set up by hand: nothing to show of them
    let hand_made = hand_made_interfaces(&rows(client, "/interface/print").await?);
    addresses.retain(|row| !field(row, "interface").is_some_and(|name| hand_made.contains(name)));
    let dhcp = Dhcp {
        servers: rows(client, "/ip/dhcp-server/print").await?,
        leases: rows(client, "/ip/dhcp-server/lease/print").await?,
    };
    let rows = device.interface_rows();
    let bridges = rows
        .iter()
        .flat_map(|(row, _)| bridge_devices(device, row))
        .collect::<BTreeSet<_>>();
    let loopbacks = bridges
        .iter()
        .filter_map(|id| device.topology().get_device_by_id(&DeviceId(*id))?.primary_ip_v4()?.addr())
        .collect::<Vec<_>>();
    let answering = answering(client, &loopbacks).await;
    let poe = poe::mismatches(device, &poe::read_ports(client).await?);
    let mut live = check(device, &rows, neighbors, &ospf, &addresses, &dhcp, &answering);
    place_poe(&mut live, &rows, poe);
    Ok(live)
}

/// Puts each PoE mismatch on the row of its port, with „PoE-Eintrag entfernen“, or „PoE
/// vervollständigen“ (`PdMissing`: „PoE“ from the port, the far end from its cable).
pub fn place_poe(live: &mut InterfaceLive, rows: &[(InterfaceRow, BTreeSet<u32>)], poe: Vec<PoeMismatch>) {
    for mismatch in poe {
        let Some(row) = rows
            .iter()
            .position(|(_, members)| members.contains(&mismatch.port.id))
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
    pub servers: Vec<Row>,
    pub leases: Vec<Row>,
}

/// [`read`] on what the device answered.
pub fn check(
    device: &DeviceAccess,
    rows: &[(InterfaceRow, BTreeSet<u32>)],
    neighbors: neighbors::Neighbors,
    ospf: &[Row],
    addresses: &[Row],
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
                .filter(|row| field(row, "interface") == Some(name))
                .filter_map(|row| field(row, "address")?.split('/').next()?.parse::<IpAddr>().ok())
                .collect::<Vec<_>>();
            device
                .interfaces()
                .iter()
                .find(|i| i.ips().iter().any(|ip| ip.addr().is_some_and(|ip| on_it.contains(&ip))))
                .map(|i| i.id().0)
        })
    };
    let ospf_device = |row: &Row| {
        let router_id = field(row, "router-id").unwrap_or_default();
        let address = field(row, "address").unwrap_or_default();
        topology
            .list_devices()
            .find(|d| d.primary_ip_v4().and_then(|ip| ip.addr()).is_some_and(|ip| ip.to_string() == router_id))
            .or_else(|| {
                topology.list_devices().find(|d| {
                    d.interfaces()
                        .iter()
                        .flat_map(|i| i.ips())
                        .any(|ip| ip.addr().is_some_and(|ip| ip.to_string() == address))
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
            .and_then(|server| dhcp.servers.iter().find(|row| field(row, "name") == Some(server)))
            .and_then(|row| field(row, "interface"))
            .and_then(netbox_interface)
            .or_else(|| {
                let address = lease.address.parse::<IpAddr>().ok()?;
                device
                    .interfaces()
                    .iter()
                    .find(|i| i.ips().iter().any(|ip| ip.net().is_some_and(|net| net.contains(&address))))
                    .map(|i| i.id().0)
            })
    };
    let mut placed = vec![false; neighbors.findings.len()];
    let live_rows = rows
        .iter()
        .map(|(row, members)| {
            let seen = members
                .iter()
                .flat_map(|id| neighbors.seen.get(id).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            let ospf_here = ospf
                .iter()
                .filter(|neighbor| {
                    field(neighbor, "interface")
                        .and_then(netbox_interface)
                        .is_some_and(|interface| members.contains(&interface))
                })
                .map(|neighbor| OspfNeighbor {
                    instance: field(neighbor, "instance").unwrap_or_default().into(),
                    address: field(neighbor, "address").unwrap_or_default().into(),
                    router_id: field(neighbor, "router-id").unwrap_or_default().into(),
                    state: field(neighbor, "state").unwrap_or_default().into(),
                    adjacent: is_adjacent(ospf, neighbor),
                    device: ospf_device(neighbor).as_ref().map(device_ref),
                })
                .collect::<Vec<_>>();
            let ospf_missing = expected_ospf(device, row)
                .into_iter()
                .filter(|expected| {
                    !ospf_here
                        .iter()
                        .any(|n| n.adjacent && n.device.as_ref().is_some_and(|d| d.id == expected.id))
                })
                .collect();
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
                .filter(|(finding, _)| {
                    port_of(finding).is_some_and(|id| members.contains(&id))
                })
                .map(|(finding, placed)| {
                    *placed = true;
                    finding.clone()
                })
                .collect();
            let mut actions = Vec::<RowAction>::new();
            let suggestions = findings
                .iter()
                .filter_map(|finding| seen_link(device, finding).or_else(|| new_device(device, finding)));
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
                ospf: ospf_here,
                ospf_missing,
                bridges,
                findings,
                device_only_addresses,
                device_only_leases,
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

/// The settings of a lease row NetBox doesn't hold, by their RouterOS names.
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

const LEASE_SETTINGS: &[&str] = &[
    "address-lists",
    "allow-dual-stack-queue",
    "always-broadcast",
    "block-access",
    "dhcp-option",
    "dhcp-option-set",
    "insert-queue-before",
    "lease-time",
    "queue-type",
    "rate-limit",
    "routes",
    "use-src-mac",
];

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
    let set = |row: &Row, name: &str| {
        field(row, name).is_some_and(|value| !value.is_empty() && !matches!(value, "false" | "none" | "0s"))
    };
    dhcp.leases
        .iter()
        .filter(|row| field(row, "dynamic") != Some("true"))
        .filter_map(|row| {
            let address = field(row, "address")?.parse::<IpAddr>().ok()?;
            if expected.contains(&address) {
                return None;
            }
            let text = |name: &str| field(row, name).filter(|v| !v.is_empty()).map(Box::from);
            let netbox = topology.ip_address_with_host(address);
            Some(DeviceOnlyLease {
                address: address.to_string().into(),
                mac: text("mac-address"),
                client_id: text("client-id"),
                server: text("server"),
                name: text("host-name")
                    .and_then(|name| client_name(&name))
                    .or_else(|| text("comment")),
                disabled: field(row, "disabled") == Some("true"),
                settings: LEASE_SETTINGS
                    .iter()
                    .filter(|name| set(row, name))
                    .map(|name| Box::from(*name))
                    .collect(),
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
const MANAGED_TYPES: [&str; 8] = ["ether", "bridge", "vlan", "vxlan", "wlan", "wifi", "w60g", "loopback"];

/// The names of the interfaces of another type (`/interface` rows): tunnels, PPPoE clients and
/// whatever else is set up by hand, whose addresses an apply keeps.
fn hand_made_interfaces(interfaces: &[Row]) -> BTreeSet<Box<str>> {
    interfaces
        .iter()
        .filter(|row| field(row, "type").is_some_and(|kind| !MANAGED_TYPES.contains(&kind)))
        .filter_map(|row| field(row, "name").map(Box::from))
        .collect()
}

/// The static addresses of the device (`/ip/address` and `/ipv6/address` rows) that NetBox
/// doesn't have on it, link-local ones left out (RouterOS makes them).
fn device_only_addresses(
    device: &DeviceAccess,
    addresses: &[Row],
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
        .filter(|row| field(row, "dynamic") != Some("true"))
        .filter_map(|row| {
            let address = field(row, "address")?.parse::<IpNet>().ok()?;
            let link_local = match address.addr() {
                IpAddr::V6(v6) => v6.is_unicast_link_local(),
                IpAddr::V4(_) => false,
            };
            if link_local || own.contains(&address.addr()) {
                return None;
            }
            let interface_name = field(row, "interface").unwrap_or_default();
            let netbox = topology.ip_address_with_host(address.addr());
            Some(DeviceOnlyAddress {
                address: address.to_string().into(),
                interface_name: interface_name.into(),
                interface: netbox_interface(interface_name),
                disabled: field(row, "disabled") == Some("true"),
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
        NeighborFindingKind::CableMissing(f) => (&f.port, f.seen.as_slice(), ActionHelper::Connection),
        NeighborFindingKind::CableElsewhere(f) => (&f.port, f.seen.as_slice(), ActionHelper::CableTakeover),
        NeighborFindingKind::OtherFarPort(f) => (&f.port, std::slice::from_ref(&f.seen), ActionHelper::CableTakeover),
        _ => return None,
    };
    let [seen] = seen else { return None };
    let (port, peer) = (port.interface.as_ref()?, seen.interface.as_ref()?);
    let topology = device.topology();
    let peer_device = topology.get_device_by_id(&DeviceId(peer.device.id))?;
    let routers = [device, &peer_device];
    if routers.iter().any(|d| !d.has_routeros() || d.is_bridge_device()) {
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
    let [seen] = f.seen.as_slice() else { return None };
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
