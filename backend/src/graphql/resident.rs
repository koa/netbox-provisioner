//! Scope: The residents' GraphQL schema (`/graphql/resident`): what a resident sees of its
//! ports and networks, and an admin of every resident's; not the admins' schema (`graphql`),
//! nor who is which role (`role`).
use crate::{
    role::Role,
    topology::{
        OwnerId, Topology, TopologyHolder,
        access::{
            AccessTopology, device::DeviceAccess, interface::InterfaceAccess, vlan::VlanAccess,
        },
        natural::naturalize_interface,
        ownership::Owned,
    },
};
use async_graphql::{
    Context, EmptyMutation, EmptySubscription, Enum, Object, Schema, SimpleObject,
};
use ipnet::IpNet;
use std::{collections::BTreeSet, sync::Arc};

#[cfg(test)]
mod test;

pub type ResidentGraphqlSchema = Schema<QueryResident, EmptyMutation, EmptySubscription>;

/// The schema the residents' endpoint serves; the request carries the user's [`Role`].
pub fn create_resident_schema(topology: TopologyHolder) -> ResidentGraphqlSchema {
    Schema::build(QueryResident { topology }, EmptyMutation, EmptySubscription).finish()
}

pub struct QueryResident {
    topology: TopologyHolder,
}

/// What the user is here.
#[derive(Enum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoleKind {
    Admin,
    Resident,
    /// neither, which the endpoint refuses before; for a request without a role
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
}

/// A device, site or VLAN by id and name.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Named {
    id: u32,
    name: Box<str>,
}

/// A port of the resident: of its device, or one NetBox gives it at another device (a socket).
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct ResidentPort {
    id: u32,
    name: Box<str>,
    label: Option<Box<str>>,
    device: Named,
    site: Option<Named>,
    /// the port is on a device of the resident
    on_own_device: bool,
    /// its untagged VLAN, if it is one of the resident's
    network: Option<Named>,
    /// it has an untagged VLAN of somebody else (the operator's), named nowhere here
    foreign_network: bool,
    /// the ports at the other end of its cable
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
    /// the access points of its WLAN group, which all send it
    access_points: u32,
}

#[Object]
impl QueryResident {
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
    let mut ports: Vec<SortedPort> = owned
        .ports
        .iter()
        .filter_map(|id| topology.get_interface_by_id(*id))
        // of its devices the sockets only: not `lo`, a radio or a bridge
        .filter(|interface| {
            interface.is_any_ethernet_port()
                || interface
                    .device()
                    .is_none_or(|device| !owned.devices.contains(&device.id()))
        })
        .filter_map(|interface| port(&owned, &interface))
        .map(|port| SortedPort {
            key: format!(
                "{}\0{}\0{}",
                port.site
                    .as_ref()
                    .map(|site| &*site.name)
                    .unwrap_or_default(),
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
    Some(Resident {
        id: owner.0,
        name: data.name.clone(),
        ports: ports.into_iter().map(|sorted| sorted.port).collect(),
        networks: networks.into(),
    })
}

/// A port with what it is sorted by.
struct SortedPort {
    key: String,
    port: ResidentPort,
}

fn port(owned: &Owned, interface: &InterfaceAccess) -> Option<ResidentPort> {
    let device = interface.device()?;
    let vlan = interface.untagged_vlan();
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
    let own_vlan = vlan
        .as_ref()
        .filter(|vlan| owned.vlans.contains(&AccessTopology::id(*vlan)));
    Some(ResidentPort {
        id: interface.id().0,
        name: interface.name().into(),
        label: interface.label().map(Box::from),
        site: device.site().map(|site| Named {
            id: AccessTopology::id(&site).0,
            name: site.name().into(),
        }),
        on_own_device: owned.devices.contains(&device.id()),
        device: named_device(&device),
        network: own_vlan.map(|vlan| Named {
            id: AccessTopology::id(vlan).0,
            name: vlan.name().unwrap_or_default().into(),
        }),
        foreign_network: vlan.is_some() && own_vlan.is_none(),
        far_ends,
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
        .wlan()
        .filter(|wlan| owned.wlans.contains(&AccessTopology::id(wlan)))
        .map(|wlan| ResidentWlan {
            id: AccessTopology::id(&wlan).0,
            ssid: wlan.ssid().unwrap_or_default().into(),
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
