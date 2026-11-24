//! Scope: What a resident's view reads from the devices: the devices in one of its networks, by
//! the leases of its DHCP servers and the clients of its WLANs; not what NetBox says
//! (`graphql::resident`), nor how a device's tables are read (`device::metrics`).
//!
//! The concept's `leases(vlan)` and `wlanClients(wlan)` in one: a device in the network is a
//! bound lease in one of its IPv4 networks, at a router with a DHCP server there (`dhcp`), and a
//! client of one of its WLANs at the WLAN group's controller (its CAPsMAN registers the CAPs'
//! clients), joined by MAC. Each device is read once, all of them at once; one that doesn't
//! answer is named, so the view doesn't pass a part for the whole.
//!
//! The devices behind the resident's ports (`portDevices`): the leases of the port's network
//! whose MAC the bridge of the switch learned at the switch port (at a socket the one its rear
//! port leads to). Only a lease of the port's own network shows, so nobody sees another's
//! devices through a port; a device without lease (a fixed address) doesn't show.
//!
//! What is up now (`liveState`): which of the devices behind the resident's elements (its own
//! devices, the switches of its ports and sockets, the access points of its WLANs) the OSPF
//! routers reach,
//! from one router's LSDB as the network map reads it; a device without OSPF of its own isn't
//! judged.
//!
//! The port a device's cable is in (its Anschluss), where it isn't the one of NetBox: the
//! router at the other end of NetBox's cable sees the device's port by name in its neighbors
//! (`seenUplinks`). Taken over by the resident, nothing happens by itself.
use super::{Named, PortKind, PortNetwork, change::acting_for};
use crate::{
    Error,
    device::{
        interface_live::client_name,
        metrics::{BridgeHost, Lease, WlanBand, WlanClient},
        neighbors::{self, SeenNeighbor},
    },
    error_info::ApiError,
    facts::Freshness,
    helper::connect_device::uplink_port,
    network_state::graphql::{NodeState, network_state},
    topology::{
        DeviceId, Topology, TopologyHolder, VlanId,
        access::{
            AccessTopology, DeviceTarget, device::DeviceAccess, front_port::FrontPortAccess,
            interface::InterfaceAccess, vlan::VlanAccess,
        },
        findings::InterfaceRef,
        ownership::{Belonging, Owned},
    },
};
use async_graphql::{Context, Object, SimpleObject, futures_util::future::join_all};
use ipnet::IpNet;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// How old the tables may be: the view asks on a click, a second click shortly after shares.
const MAX_AGE: Duration = Duration::from_secs(10);

/// The devices in one of the resident's networks, as the routers and controllers see them now.
#[derive(SimpleObject, Debug, Clone)]
pub struct NetworkDevices {
    /// by address, those without one (a WLAN client without lease) last
    devices: Box<[NetworkDevice]>,
    /// the routers and controllers that didn't answer: their devices are missing
    unreachable: Box<[Named]>,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct NetworkDevice {
    mac_address: Box<str>,
    /// its IPv4 from a lease
    address: Option<Box<str>>,
    /// the name its DHCP client sent
    hostname: Option<Box<str>>,
    /// connected over one of the network's WLANs
    wlan: Option<WlanConnection>,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct WlanConnection {
    ssid: Box<str>,
    /// the identity of the access point it is on; empty where the controller doesn't say
    access_point: Box<str>,
    /// dBm
    signal: i32,
    /// none where the controller doesn't say
    band: Option<WlanBand>,
}

/// The devices behind the resident's ports, as the switches and routers see them now.
#[derive(SimpleObject, Debug, Clone)]
pub struct PortDevices {
    /// the ports in one of its networks
    ports: Box<[PortHosts]>,
    /// the switches and routers that didn't answer: devices behind ports are missing
    unreachable: Box<[Named]>,
}

/// The devices behind one of the resident's ports: the leases of its network whose MAC the
/// bridge learned there.
#[derive(SimpleObject, Debug, Clone)]
pub struct PortHosts {
    kind: PortKind,
    /// the interface or front port, as in the resident's ports
    port: u32,
    /// by address
    devices: Box<[NetworkDevice]>,
}

/// A port of the resident in one of its networks, and the switch port its devices are learned
/// at.
struct Watched {
    kind: PortKind,
    id: u32,
    switch_port: InterfaceAccess,
    switch: DeviceAccess,
    vlan: VlanId,
}

/// Which tables of a device are asked.
#[derive(Default)]
struct Wanted {
    leases: bool,
    clients: bool,
    hosts: bool,
}

/// Which devices behind the resident's elements are up, as the OSPF routers see them.
#[derive(SimpleObject, Debug, Clone)]
pub struct LiveState {
    /// false: no router answered, nothing is known
    known: bool,
    /// its own devices, each with whether it is reached; one without OSPF of its own is
    /// missing
    devices: Box<[DeviceLive]>,
    /// its ports and sockets, each with whether the switch carrying it is reached (a socket's:
    /// the one its rear port leads to); one at a switch without OSPF of its own is missing
    ports: Box<[PortLive]>,
    /// its WLANs, each with the access points that aren't reached
    wlans: Box<[WlanLive]>,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct DeviceLive {
    device: Named,
    reached: bool,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct PortLive {
    kind: PortKind,
    id: u32,
    switch: Named,
    reached: bool,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct WlanLive {
    wlan: u32,
    /// its group's access points, all
    access_points: u32,
    /// those not reached (one without OSPF of its own counts as reached)
    unreached: Box<[Named]>,
}

/// Where the routers see the resident's devices plugged in, where it isn't NetBox's Anschluss.
#[derive(SimpleObject, Debug, Clone)]
pub struct SeenUplinks {
    devices: Box<[SeenUplink]>,
    /// the routers that didn't answer: their devices are missing
    unreachable: Box<[Named]>,
}

/// A device of the resident whose cable is in `port`, not in its Anschluss of NetBox.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct SeenUplink {
    device: u32,
    port: Named,
}

/// A device of the resident and the router port NetBox's cable at its Anschluss leads to.
struct Linked {
    device: DeviceAccess,
    uplink: InterfaceAccess,
    router_port: InterfaceAccess,
}

/// A router at the far end of Anschlüsse, the devices linked to it.
struct RouterOf {
    router: DeviceAccess,
    linked: Vec<Linked>,
}

/// A WLAN of the network and the controller registering its clients.
struct WlanAt {
    ssid: Box<str>,
    controller: DeviceAccess,
}

/// A router's DHCP server in the network: its device and the networks it serves there.
struct Server {
    device: DeviceAccess,
    networks: Vec<IpNet>,
}

pub struct QueryLive {
    topology: TopologyHolder,
}

impl QueryLive {
    pub fn new(topology: TopologyHolder) -> Self {
        QueryLive { topology }
    }
}

#[Object]
impl QueryLive {
    /// The devices in `resident`'s network (VLAN) `vlan`: leases and WLAN clients.
    async fn network_devices(
        &self,
        context: &Context<'_>,
        resident: u32,
        vlan: u32,
    ) -> Result<NetworkDevices, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        if !topology.owned_by(owner).vlans.contains(&VlanId(vlan)) {
            return Err(Error::NotOwned {
                what: Belonging::Vlan,
                id: vlan,
            }
            .into());
        }
        let vlan = topology
            .list_vlans()
            .find(|other| other.id == VlanId(vlan))
            .ok_or(Error::UnknownVlan(vlan))?;
        let servers = dhcp_servers(&vlan);
        let wlans = vlan
            .all_wlans()
            .filter_map(|wlan| {
                Some(WlanAt {
                    ssid: wlan.ssid()?.into(),
                    controller: wlan.wlan_group()?.controller()?,
                })
            })
            .collect::<Vec<_>>();
        let mut asked = BTreeMap::<DeviceId, DeviceAccess>::new();
        for device in servers
            .values()
            .map(|server| server.device.clone())
            .chain(wlans.iter().map(|wlan| wlan.controller.clone()))
        {
            asked.entry(device.id()).or_insert(device);
        }
        let read = join_all(asked.values().map(|device| {
            let wanted = Wanted {
                leases: servers.contains_key(&device.id()),
                clients: wlans.iter().any(|wlan| wlan.controller.id() == device.id()),
                hosts: false,
            };
            async move { (device.clone(), read(device, &wanted).await) }
        }))
        .await;
        let mut unreachable = Vec::new();
        let mut leases = Vec::<Lease>::new();
        let mut clients = Vec::<WlanClient>::new();
        for (device, answer) in read {
            match answer {
                Ok(Tables {
                    leases: device_leases,
                    clients: device_clients,
                    ..
                }) => {
                    let networks = servers
                        .get(&device.id())
                        .map(|server| server.networks.as_slice())
                        .unwrap_or_default();
                    leases.extend(
                        device_leases.into_iter().filter(|lease| {
                            networks.iter().any(|net| net.contains(&lease.address))
                        }),
                    );
                    clients.extend(device_clients.into_iter().filter(|client| {
                        wlans.iter().any(|wlan| {
                            wlan.controller.id() == device.id() && wlan.ssid == client.ssid
                        })
                    }));
                }
                Err(_) => unreachable.push(Named {
                    id: device.id().0,
                    name: device.name().into(),
                }),
            }
        }
        Ok(NetworkDevices {
            devices: joined(&leases, &clients),
            unreachable: unreachable.into(),
        })
    }
    /// The devices behind `resident`'s ports in one of its networks: each switch and router read
    /// once, all at once.
    async fn port_devices(
        &self,
        context: &Context<'_>,
        resident: u32,
    ) -> Result<PortDevices, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let owned = topology.owned_by(owner);
        let watched = watched_ports(&topology, &owned);
        let mut servers = BTreeMap::<VlanId, BTreeMap<DeviceId, Server>>::new();
        for port in &watched {
            servers
                .entry(port.vlan)
                .or_insert_with(|| dhcp_servers(&VlanAccess::create(topology.clone(), port.vlan)));
        }
        let mut asked = BTreeMap::<DeviceId, (DeviceAccess, Wanted)>::new();
        for port in &watched {
            asked
                .entry(port.switch.id())
                .or_insert_with(|| (port.switch.clone(), Wanted::default()))
                .1
                .hosts = true;
        }
        for server in servers.values().flat_map(BTreeMap::values) {
            asked
                .entry(server.device.id())
                .or_insert_with(|| (server.device.clone(), Wanted::default()))
                .1
                .leases = true;
        }
        let read = join_all(
            asked
                .values()
                .map(|(device, wanted)| async move { (device.id(), read(device, wanted).await) }),
        )
        .await;
        let mut unreachable = Vec::new();
        let mut tables = BTreeMap::<DeviceId, Tables>::new();
        for (device, answer) in read {
            match answer {
                Ok(answer) => {
                    tables.insert(device, answer);
                }
                Err(_) => {
                    let (device, _) = &asked[&device];
                    unreachable.push(Named {
                        id: device.id().0,
                        name: device.name().into(),
                    });
                }
            }
        }
        let ports = watched
            .iter()
            .map(|port| {
                let hosts = tables
                    .get(&port.switch.id())
                    .map(|tables| &*tables.hosts)
                    .unwrap_or_default();
                let leases = servers
                    .get(&port.vlan)
                    .into_iter()
                    .flat_map(|servers| servers.values())
                    .flat_map(|server| {
                        tables
                            .get(&server.device.id())
                            .map(|tables| &*tables.leases)
                            .unwrap_or_default()
                            .iter()
                            .filter(|lease| {
                                server
                                    .networks
                                    .iter()
                                    .any(|net| net.contains(&lease.address))
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                PortHosts {
                    kind: port.kind,
                    port: port.id,
                    devices: behind(port.switch_port.name(), hosts, &leases),
                }
            })
            .collect();
        Ok(PortDevices {
            ports,
            unreachable: unreachable.into(),
        })
    }
    /// Which devices behind `resident`'s elements are up: one LSDB read for its sites.
    async fn live_state(
        &self,
        context: &Context<'_>,
        resident: u32,
    ) -> Result<LiveState, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let owned = topology.owned_by(owner);
        let sites = super::resident_sites(&topology, &owned);
        let in_sites = topology
            .list_devices()
            .filter(|device| {
                device
                    .site()
                    .is_some_and(|site| sites.contains(&AccessTopology::id(&site)))
            })
            .collect::<Vec<_>>();
        let Some(states) = network_state(in_sites).await.states() else {
            return Ok(LiveState {
                known: false,
                devices: Box::default(),
                ports: Box::default(),
                wlans: Box::default(),
            });
        };
        // reached; none where the map doesn't judge it
        let reached = |device: &DeviceAccess| match states.get(&device.id().0)? {
            NodeState::Reached => Some(true),
            NodeState::Isolated | NodeState::Missing => Some(false),
            NodeState::NotOspf => None,
        };
        let named = |device: &DeviceAccess| Named {
            id: device.id().0,
            name: device.name().into(),
        };
        let devices = owned
            .devices
            .iter()
            .filter_map(|id| topology.get_device_by_id(id))
            .filter_map(|device| {
                Some(DeviceLive {
                    reached: reached(&device)?,
                    device: named(&device),
                })
            })
            .collect();
        let port = |kind: PortKind, id: u32, switch_port: Option<InterfaceAccess>| {
            let switch = switch_port?.device()?;
            Some(PortLive {
                kind,
                id,
                reached: reached(&switch)?,
                switch: named(&switch),
            })
        };
        let ports = owned
            .ports
            .iter()
            .filter_map(|id| port(PortKind::Interface, id.0, topology.get_interface_by_id(*id)))
            .chain(owned.sockets.iter().filter_map(|id| {
                let socket = FrontPortAccess::create(topology.clone(), *id);
                port(PortKind::Socket, id.0, socket.interface_behind())
            }))
            .collect();
        let wlans = owned
            .vlans
            .iter()
            .flat_map(|id| {
                VlanAccess::create(topology.clone(), *id)
                    .all_wlans()
                    .collect::<Vec<_>>()
            })
            .filter(|wlan| owned.wlans.contains(&AccessTopology::id(wlan)))
            .map(|wlan| {
                let aps = wlan
                    .wlan_group()
                    .map(|group| group.aps())
                    .unwrap_or_default();
                WlanLive {
                    wlan: AccessTopology::id(&wlan).0,
                    access_points: aps.len() as u32,
                    unreached: aps
                        .iter()
                        .filter(|ap| reached(ap) == Some(false))
                        .map(named)
                        .collect(),
                }
            })
            .collect();
        Ok(LiveState {
            known: true,
            devices,
            ports,
            wlans,
        })
    }
    /// Where the routers see `resident`'s RouterOS devices plugged in, where it isn't their
    /// Anschluss in NetBox; each router read once, all at once.
    async fn seen_uplinks(
        &self,
        context: &Context<'_>,
        resident: u32,
    ) -> Result<SeenUplinks, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let mut routers = BTreeMap::<DeviceId, RouterOf>::new();
        for device in topology
            .owned_by(owner)
            .devices
            .iter()
            .filter_map(|id| topology.get_device_by_id(id))
            .filter(DeviceAccess::has_routeros)
        {
            let Some(uplink) = uplink_port(&device).filter(|port| port.cable().is_some()) else {
                continue;
            };
            let Some((router, router_port)) = uplink
                .connected_interfaces()
                .into_iter()
                .find_map(|far| Some((far.device().filter(DeviceAccess::has_routeros)?, far)))
            else {
                continue;
            };
            routers
                .entry(router.id())
                .or_insert_with(|| RouterOf {
                    router,
                    linked: Vec::new(),
                })
                .linked
                .push(Linked {
                    device,
                    uplink,
                    router_port,
                });
        }
        let seen = join_all(routers.values().map(|of| seen_by(&of.router))).await;
        let mut devices = Vec::new();
        let mut unreachable = Vec::new();
        for (RouterOf { router, linked }, seen) in routers.into_values().zip(seen) {
            let Ok(seen) = seen else {
                unreachable.push(Named {
                    id: router.id().0,
                    name: router.name().into(),
                });
                continue;
            };
            devices.extend(linked.iter().filter_map(|linked| {
                let at = seen.get(&linked.router_port.id().0)?;
                let port = plugged_in(linked.device.id().0, linked.uplink.id().0, at)?;
                Some(SeenUplink {
                    device: linked.device.id().0,
                    port: Named {
                        id: port.id,
                        name: port.name,
                    },
                })
            }));
        }
        Ok(SeenUplinks {
            devices: devices.into(),
            unreachable: unreachable.into(),
        })
    }
}

/// Who `router` sees at its NetBox ports (by id); an error where it doesn't answer.
async fn seen_by(router: &DeviceAccess) -> Result<BTreeMap<u32, Vec<SeenNeighbor>>, Error> {
    let accessible = router
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::MissingCredentials)?;
    let table = neighbors::read_table(&accessible.reader().at(Freshness::MaxAge(MAX_AGE))).await?;
    Ok(neighbors::evaluate(router, &table).seen)
}

/// The port of `device` the router sees at its end of the cable (`seen`), where it is a NetBox
/// port other than its Anschluss `uplink`.
fn plugged_in(device: u32, uplink: u32, seen: &[SeenNeighbor]) -> Option<InterfaceRef> {
    seen.iter()
        .filter(|neighbor| neighbor.device.as_ref().is_some_and(|d| d.id == device))
        .find_map(|neighbor| neighbor.interface.clone())
        .filter(|port| port.id != uplink)
}

/// The interfaces that are `vlan` (its VLAN interface, a port with it untagged) with a DHCP
/// server, by device: the routers handing out its leases.
fn dhcp_servers(vlan: &VlanAccess) -> BTreeMap<DeviceId, Server> {
    let topology = vlan.topology();
    let mut servers = BTreeMap::<DeviceId, Server>::new();
    for interface in vlan
        .data()
        .map(|data| data.terminations.clone())
        .unwrap_or_default()
        .iter()
        .map(|id| InterfaceAccess::create(topology.clone(), *id))
        .filter(|interface| {
            interface
                .data()
                .is_some_and(|data| data.vlan == Some(vlan.id))
        })
        .filter(InterfaceAccess::is_enable_dhcp_server)
    {
        let Some(device) = interface.device() else {
            continue;
        };
        let networks = interface
            .ips()
            .iter()
            .filter_map(|ip| ip.net())
            .filter(|net| net.addr().is_ipv4())
            .map(|net| net.trunc())
            .collect::<Vec<_>>();
        servers
            .entry(device.id())
            .or_insert_with(|| Server {
                device: device.clone(),
                networks: Vec::new(),
            })
            .networks
            .extend(networks);
    }
    servers
}

/// The resident's ports in one of its networks at a RouterOS switch: its interfaces, and its
/// sockets by the switch port their rear port leads to.
fn watched_ports(topology: &Arc<Topology>, owned: &Owned) -> Vec<Watched> {
    let at = |kind: PortKind, id: u32, switch_port: InterfaceAccess| {
        let vlan = AccessTopology::id(&PortNetwork::of(&switch_port).vlan?);
        let switch = switch_port.device().filter(DeviceAccess::has_routeros)?;
        owned.vlans.contains(&vlan).then_some(Watched {
            kind,
            id,
            switch_port,
            switch,
            vlan,
        })
    };
    let interfaces = owned
        .ports
        .iter()
        .filter_map(|id| topology.get_interface_by_id(*id))
        .filter_map(|interface| at(PortKind::Interface, interface.id().0, interface));
    let sockets = owned.sockets.iter().filter_map(|id| {
        let socket = FrontPortAccess::create(topology.clone(), *id);
        at(PortKind::Socket, id.0, socket.interface_behind()?)
    });
    interfaces.chain(sockets).collect()
}

/// The devices learned at `port` (by its name or default name) among `hosts`, by their lease.
fn behind(port: &str, hosts: &[BridgeHost], leases: &[Lease]) -> Box<[NetworkDevice]> {
    let learned = hosts
        .iter()
        .filter(|host| &*host.port == port || host.default_name.as_deref() == Some(port))
        .map(|host| host.mac_address)
        .collect::<Vec<_>>();
    let leases = leases
        .iter()
        .filter(|lease| learned.contains(&lease.mac_address))
        .cloned()
        .collect::<Vec<_>>();
    joined(&leases, &[])
}

/// What one device answered.
#[derive(Default)]
struct Tables {
    leases: Box<[Lease]>,
    clients: Box<[WlanClient]>,
    hosts: Box<[BridgeHost]>,
}

/// The leases, WLAN clients and bridge hosts of `device`, as asked; an error where it doesn't answer.
async fn read(device: &DeviceAccess, wanted: &Wanted) -> Result<Tables, Error> {
    // none: no credentials or no way to it, as good as not answering
    let accessible = device
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::MissingCredentials)?;
    let freshness = Freshness::MaxAge(MAX_AGE);
    // one after the other: a device's tables are never read at once
    let leases = match wanted.leases {
        true => accessible.leases(freshness).await?,
        false => Box::default(),
    };
    let clients = match wanted.clients {
        true => accessible.wlan_clients(freshness).await?,
        false => Box::default(),
    };
    let hosts = match wanted.hosts {
        true => accessible.bridge_hosts(freshness).await?,
        false => Box::default(),
    };
    Ok(Tables {
        leases,
        clients,
        hosts,
    })
}

/// Every MAC once: its lease, the WLAN it is on.
fn joined(leases: &[Lease], clients: &[WlanClient]) -> Box<[NetworkDevice]> {
    let mut devices = BTreeMap::<String, NetworkDevice>::new();
    for lease in leases {
        let mac = lease.mac_address.to_string();
        devices.insert(
            mac.clone(),
            NetworkDevice {
                mac_address: mac.into(),
                address: Some(lease.address.to_string().into()),
                hostname: lease.hostname.as_deref().and_then(client_name),
                wlan: None,
            },
        );
    }
    for client in clients {
        let mac = client.mac_address.to_string();
        let connection = WlanConnection {
            ssid: client.ssid.clone(),
            access_point: client.ap.clone(),
            signal: client.signal.into(),
            band: client.band,
        };
        devices
            .entry(mac.clone())
            .or_insert_with(|| NetworkDevice {
                mac_address: mac.into(),
                address: None,
                hostname: None,
                wlan: None,
            })
            .wlan = Some(connection);
    }
    let mut devices = devices.into_values().collect::<Vec<_>>();
    devices.sort_by_key(|device| {
        (
            device.address.is_none(),
            device
                .address
                .as_deref()
                .and_then(|address| address.parse::<std::net::IpAddr>().ok()),
        )
    });
    devices.into()
}

#[cfg(test)]
mod test;
