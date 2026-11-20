//! Scope: The residents' GraphQL schema (`/graphql/resident`): what a resident sees of its
//! ports and networks, and an admin of every resident's; not what they change (`change`), the
//! admins' schema (`graphql`), nor who is which role (`role`).
use crate::helper::connect_device::uplink_port;
use crate::{
    role::Role,
    topology::{
        OwnerId, SiteId, Topology, TopologyHolder, WlanAuth,
        access::{
            AccessTopology, cable::CablePortAccess, device::DeviceAccess,
            front_port::FrontPortAccess, interface::InterfaceAccess, site::SiteAccess,
            vlan::VlanAccess,
        },
        natural::{naturalize, naturalize_interface},
        ownership::Owned,
    },
};
use async_graphql::{Context, EmptySubscription, Enum, MergedObject, Object, Schema, SimpleObject};
use ipnet::IpNet;
use std::{collections::BTreeSet, sync::Arc};

pub mod change;
pub mod live;
#[cfg(test)]
mod test;

pub type ResidentGraphqlSchema = Schema<QueryResident, change::MutationResident, EmptySubscription>;

/// The schema the residents' endpoint serves; the request carries the user's [`Role`] and
/// `UserInfo`.
pub fn create_resident_schema(topology: TopologyHolder) -> ResidentGraphqlSchema {
    let query = QueryResident(
        QueryView {
            topology: topology.clone(),
        },
        change::QueryChange::new(topology.clone()),
        live::QueryLive::new(topology.clone()),
    );
    Schema::build(
        query,
        change::MutationResident::new(topology),
        EmptySubscription,
    )
    .finish()
}

/// What a resident sees (`QueryView`) and the plans of its changes (`change`).
#[derive(MergedObject)]
pub struct QueryResident(QueryView, change::QueryChange, live::QueryLive);

pub struct QueryView {
    topology: TopologyHolder,
}

/// What the user is here.
#[derive(Enum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoleKind {
    Admin,
    Resident,
    /// neither: sees nothing
    Nobody,
}

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Me {
    role: RoleKind,
}

/// A resident (a NetBox owner) with what it has.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Resident {
    id: u32,
    name: Box<str>,
    /// its ports, by site, device and port name: those NetBox gives it at other devices and the
    /// Ethernet ports of its devices
    ports: Box<[ResidentPort]>,
    /// its VLANs, by name
    networks: Box<[ResidentNetwork]>,
    /// its devices, by name
    devices: Box<[ResidentDevice]>,
    /// the rooms (NetBox locations) of the sites it has ports or devices in, by site and in
    /// the order of the building: where its devices can stand
    locations: Box<[ResidentLocation]>,
}

/// A NetBox location a device of the resident can stand in.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentLocation {
    id: u32,
    /// its name and the names of the ones it is in, the outermost first
    path: Box<[Box<str>]>,
    site: Option<Named>,
}

/// A device of the resident.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentDevice {
    id: u32,
    name: Box<str>,
    /// it runs RouterOS: the provisioner links and configures it
    routeros: bool,
    /// its Anschluss with a cable: the port its link goes out of
    uplink: Option<Box<str>>,
    /// its Ethernet ports, one of which can be its Anschluss (RouterOS devices only)
    ports: Box<[Named]>,
    /// the NetBox location (room) it stands in
    location: Option<u32>,
}

/// A device, site or VLAN by id and name.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Named {
    id: u32,
    name: Box<str>,
}

/// What a row of the resident's ports is in NetBox.
#[derive(Enum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// an interface: a port of its device, or of another device given to it
    Interface,
    /// a front port: a socket in its flat
    Socket,
}

/// A port of the resident: a socket in its flat, a port of its device, or one NetBox gives it
/// at another device.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentPort {
    /// the NetBox id of the interface or front port (`kind`)
    id: u32,
    kind: PortKind,
    name: Box<str>,
    label: Option<Box<str>>,
    device: Named,
    site: Option<Named>,
    /// the NetBox location of its device and the ones that location is in, the outermost
    /// first (a floor, a flat, a room); empty without one
    location: Box<[Box<str>]>,
    /// the port is on a device of the resident
    on_own_device: bool,
    /// its untagged VLAN or its bridge's, if it is one of the resident's; of a socket that of
    /// the switch port its rear port leads to
    network: Option<Named>,
    /// it has a network of somebody else (the operator's VLAN, or addresses without the
    /// resident's VLAN), named nowhere here
    foreign_network: bool,
    /// the ports at the other end of its cable; of a socket the device plugged in
    far_ends: Box<[FarEnd]>,
}

/// The other end of a cable: a port of a device.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct FarEnd {
    device: Named,
    port: Box<str>,
    /// the device is one of the resident's
    own_device: bool,
}

/// A VLAN of the resident: a network with at most one WLAN.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentNetwork {
    id: u32,
    name: Box<str>,
    vid: u16,
    /// the networks of its addresses in NetBox (`192.168.5.0/24`)
    subnets: Box<[Box<str>]>,
    wlans: Box<[ResidentWlan]>,
}

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentWlan {
    id: u32,
    ssid: Box<str>,
    /// switched on: its APs send it
    enabled: bool,
    /// its WPA personal key, for showing it to its resident; none for an open WLAN (only an
    /// admin changes the security)
    psk: Option<Box<str>>,
    /// the access points of its WLAN group, which all send it
    access_points: u32,
}

#[Object]
impl QueryView {
    async fn me(&self, context: &Context<'_>) -> Me {
        let role = match context.data_opt::<Role>() {
            Some(Role::Admin) => RoleKind::Admin,
            Some(Role::Resident { .. }) => RoleKind::Resident,
            Some(Role::Nobody) | None => RoleKind::Nobody,
        };
        Me { role }
    }
    /// The residents the user may see: a resident its own, an admin every one NetBox names
    /// at a device, port or VLAN.
    async fn residents(&self, context: &Context<'_>) -> Box<[Resident]> {
        let Some(topology) = self.topology.current().await else {
            return Box::default();
        };
        let owners: Box<[OwnerId]> = match context.data_opt::<Role>() {
            Some(Role::Admin) => topology.owners().map(|(id, _)| id).collect(),
            Some(Role::Resident { owners }) => owners.clone(),
            Some(Role::Nobody) | None => Box::default(),
        };
        let mut residents: Vec<Resident> = owners
            .iter()
            .filter_map(|owner| resident(&topology, *owner))
            .collect();
        residents.sort_by(|a, b| a.name.cmp(&b.name));
        residents.into()
    }
}

/// What `owner` has, as the resident UI shows it; none for an owner NetBox doesn't name.
pub fn resident(topology: &Arc<Topology>, owner: OwnerId) -> Option<Resident> {
    let (_, data) = topology.owners().find(|(id, _)| *id == owner)?;
    let owned = topology.owned_by(owner);
    let sockets = owned
        .sockets
        .iter()
        .map(|id| FrontPortAccess::create(topology.clone(), *id))
        .filter_map(|socket| socket_port(&owned, &socket));
    let mut ports: Vec<SortedPort> = owned
        .ports
        .iter()
        .filter_map(|id| topology.get_interface_by_id(*id))
        // of its devices the Ethernet ports only (not `lo`, a radio or a bridge), and not the
        // one in a socket of its own: that one is shown at the socket
        .filter(|interface| {
            let own_device = interface
                .device()
                .is_some_and(|device| owned.devices.contains(&device.id()));
            !own_device || (interface.is_any_ethernet_port() && !in_own_socket(&owned, interface))
        })
        .filter_map(|interface| port(&owned, &interface))
        .chain(sockets)
        .map(|port| SortedPort {
            key: format!(
                "{}\0{}\0{}\0{}",
                port.site
                    .as_ref()
                    .map(|site| &*site.name)
                    .unwrap_or_default(),
                location_key(&port.location),
                port.device.name,
                naturalize_interface(&port.name)
            ),
            port,
        })
        .collect();
    ports.sort_by(|a, b| a.key.cmp(&b.key));
    let mut networks: Vec<ResidentNetwork> = owned
        .vlans
        .iter()
        .map(|vlan| VlanAccess::create(topology.clone(), *vlan))
        .filter_map(|vlan| network(&owned, &vlan))
        .collect();
    networks.sort_by(|a, b| a.name.cmp(&b.name));
    let mut devices: Vec<ResidentDevice> = owned
        .devices
        .iter()
        .filter_map(|id| topology.get_device_by_id(id))
        .map(|device| ResidentDevice {
            id: device.id().0,
            name: device.name().into(),
            routeros: device.has_routeros(),
            uplink: device
                .has_routeros()
                .then(|| uplink_port(&device))
                .flatten()
                .filter(|port| port.cable().is_some())
                .map(|port| port.name().into()),
            ports: match device.has_routeros() {
                true => ethernet_ports(&device),
                false => Box::default(),
            },
            location: device.location_id(),
        })
        .collect();
    devices.sort_by_cached_key(|device| naturalize(&device.name));
    let sites = resident_sites(topology, &owned);
    let mut locations: Vec<(String, ResidentLocation)> = topology
        .locations()
        .iter()
        .filter(|(_, location)| location.site.is_some_and(|site| sites.contains(&site)))
        .map(|(id, location)| {
            let site = location.site.map(|id| {
                let site = SiteAccess::create(topology.clone(), id);
                Named {
                    id: id.0,
                    name: site.name().into(),
                }
            });
            let key = format!(
                "{}\0{}",
                site.as_ref().map(|site| &*site.name).unwrap_or_default(),
                location_key(&location.path)
            );
            let location = ResidentLocation {
                id: *id,
                path: location.path.clone(),
                site,
            };
            (key, location)
        })
        .collect();
    locations.sort_by(|a, b| a.0.cmp(&b.0));
    Some(Resident {
        id: owner.0,
        name: data.name.clone(),
        ports: ports.into_iter().map(|sorted| sorted.port).collect(),
        networks: networks.into(),
        devices: devices.into(),
        locations: locations
            .into_iter()
            .map(|(_, location)| location)
            .collect(),
    })
}

/// The sites `owned` has ports, sockets or devices in.
pub(crate) fn resident_sites(topology: &Arc<Topology>, owned: &Owned) -> BTreeSet<SiteId> {
    let of = |device: Option<DeviceAccess>| device.and_then(|device| device.site());
    owned
        .devices
        .iter()
        .map(|id| of(topology.get_device_by_id(id)))
        .chain(owned.ports.iter().map(|id| {
            of(topology
                .get_interface_by_id(*id)
                .and_then(|port| port.device()))
        }))
        .chain(
            owned
                .sockets
                .iter()
                .map(|id| of(FrontPortAccess::create(topology.clone(), *id).device())),
        )
        .flatten()
        .map(|site| AccessTopology::id(&site))
        .collect()
}

/// The Ethernet ports of `device`, by name.
fn ethernet_ports(device: &DeviceAccess) -> Box<[Named]> {
    let mut ports = device
        .interfaces()
        .iter()
        .filter(|port| port.is_ethernet_port())
        .map(|port| Named {
            id: port.id().0,
            name: port.name().into(),
        })
        .collect::<Vec<_>>();
    ports.sort_by_cached_key(|port| naturalize_interface(&port.name));
    ports.into()
}

/// A port with what it is sorted by.
/// The order of locations: by name, numbers by value („2. OG“ before „10. OG“), case
/// ignored; a location before the ones in it.
fn location_key(location: &[Box<str>]) -> String {
    location
        .iter()
        .map(|name| naturalize(&name.to_lowercase()))
        .collect::<Vec<_>>()
        .join("\u{1}")
}

struct SortedPort {
    key: String,
    port: ResidentPort,
}

fn port(owned: &Owned, interface: &InterfaceAccess) -> Option<ResidentPort> {
    let device = interface.device()?;
    let network = PortNetwork::of(interface);
    let far_ends = interface
        .connected_interfaces()
        .iter()
        .filter_map(|far| {
            let far_device = far.device()?;
            Some(FarEnd {
                device: named_device(&far_device),
                port: far.name().into(),
                own_device: owned.devices.contains(&far_device.id()),
            })
        })
        .collect();
    Some(ResidentPort {
        id: interface.id().0,
        kind: PortKind::Interface,
        name: interface.name().into(),
        label: interface.label().map(Box::from),
        site: device.site().map(|site| Named {
            id: AccessTopology::id(&site).0,
            name: site.name().into(),
        }),
        location: device.location().into(),
        on_own_device: owned.devices.contains(&device.id()),
        device: named_device(&device),
        network: network.own(owned),
        foreign_network: network.foreign(owned),
        far_ends,
    })
}

/// A socket: the device plugged in at its front, the network of the switch port its rear port
/// leads to.
fn socket_port(owned: &Owned, socket: &FrontPortAccess) -> Option<ResidentPort> {
    let data = socket.data()?;
    let device = socket.device()?;
    let far_ends = interfaces_at(CablePortAccess::FrontPort(socket.clone()))
        .iter()
        .filter_map(|far| far_end(owned, far))
        .collect();
    let network = switch_port(socket)
        .as_ref()
        .map(PortNetwork::of)
        .unwrap_or_default();
    Some(ResidentPort {
        id: AccessTopology::id(socket).0,
        kind: PortKind::Socket,
        name: data.name.clone(),
        label: Some(data.label.clone()).filter(|label| !label.is_empty()),
        site: device.site().map(|site| Named {
            id: AccessTopology::id(&site).0,
            name: site.name().into(),
        }),
        location: device.location().into(),
        on_own_device: false,
        device: named_device(&device),
        network: network.own(owned),
        foreign_network: network.foreign(owned),
        far_ends,
    })
}

/// The network on a port: its untagged VLAN, else its bridge's; a port or bridge with
/// addresses but no VLAN is a network too, though nobody's VLAN.
#[derive(Default)]
struct PortNetwork {
    vlan: Option<VlanAccess>,
    addressed: bool,
}

impl PortNetwork {
    fn of(port: &InterfaceAccess) -> Self {
        let bridge = port.bridge();
        let vlan = port
            .untagged_vlan()
            .or_else(|| bridge.as_ref().and_then(|bridge| bridge.untagged_vlan()));
        let addressed = !port.ips().is_empty()
            || bridge
                .as_ref()
                .is_some_and(|bridge| !bridge.ips().is_empty());
        PortNetwork { vlan, addressed }
    }
    /// the VLAN, if it is the resident's
    fn own(&self, owned: &Owned) -> Option<Named> {
        let vlan = self.vlan.as_ref()?;
        let id = AccessTopology::id(vlan);
        owned.vlans.contains(&id).then(|| Named {
            id: id.0,
            name: vlan.name().unwrap_or_default().into(),
        })
    }
    /// a network there, but not the resident's
    fn foreign(&self, owned: &Owned) -> bool {
        self.own(owned).is_none() && (self.vlan.is_some() || self.addressed)
    }
}

/// The interfaces the cables from `start` end at, through patch panels.
/// The switch port behind the wall of `socket`: the first interface the rear port's cables
/// reach. It carries the socket's network.
pub(crate) fn switch_port(socket: &FrontPortAccess) -> Option<InterfaceAccess> {
    let rear = socket.rear_port()?;
    interfaces_at(CablePortAccess::RearPort(rear))
        .into_vec()
        .into_iter()
        .next()
}

pub(crate) fn interfaces_at(start: CablePortAccess) -> Box<[InterfaceAccess]> {
    let mut found = Vec::new();
    start.walk_cable(&mut |path| {
        if let CablePortAccess::Interface(interface) = path.far_port() {
            found.push(interface.clone());
        }
    });
    found.into()
}

/// Whether the cable of `interface` (a port of the resident's device) goes into one of its
/// sockets: then it is the device's connection, shown at the socket.
fn in_own_socket(owned: &Owned, interface: &InterfaceAccess) -> bool {
    interface
        .cable_port()
        .attached_cable_segments()
        .iter()
        .any(|segment| match &segment.far {
            CablePortAccess::FrontPort(front) => owned.sockets.contains(&AccessTopology::id(front)),
            _ => false,
        })
}

fn far_end(owned: &Owned, far: &InterfaceAccess) -> Option<FarEnd> {
    let far_device = far.device()?;
    Some(FarEnd {
        device: named_device(&far_device),
        port: far.name().into(),
        own_device: owned.devices.contains(&far_device.id()),
    })
}

fn named_device(device: &DeviceAccess) -> Named {
    Named {
        id: device.id().0,
        name: device.name().into(),
    }
}

fn network(owned: &Owned, vlan: &VlanAccess) -> Option<ResidentNetwork> {
    let id = AccessTopology::id(vlan);
    let data = vlan.data()?;
    // the addresses of the interfaces that are this VLAN (its VLAN interfaces, a port with it
    // untagged), not of a trunk carrying it tagged
    let subnets: BTreeSet<IpNet> = data
        .terminations
        .iter()
        .map(|interface| InterfaceAccess::create(vlan.topology(), *interface))
        .filter(|interface| interface.data().is_some_and(|data| data.vlan == Some(id)))
        .flat_map(|interface| interface.ips())
        .filter_map(|address| address.net())
        .map(|net| net.trunc())
        .collect();
    let wlans = vlan
        .all_wlans()
        .filter(|wlan| owned.wlans.contains(&AccessTopology::id(wlan)))
        .map(|wlan| ResidentWlan {
            id: AccessTopology::id(&wlan).0,
            ssid: wlan.ssid().unwrap_or_default().into(),
            enabled: wlan.enabled(),
            psk: match wlan.auth() {
                Some(WlanAuth::Wpa(wpa)) => Some(wpa.psk().into()),
                _ => None,
            },
            access_points: wlan
                .wlan_group()
                .map_or(0, |group| group.aps().len() as u32),
        })
        .collect();
    Some(ResidentNetwork {
        id: id.0,
        name: data.name.clone(),
        vid: data.vlan_id,
        subnets: subnets.iter().map(|net| net.to_string().into()).collect(),
        wlans,
    })
}
