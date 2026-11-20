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
//! The port a device's cable is in (its Anschluss), where it isn't the one of NetBox: the
//! router at the other end of NetBox's cable sees the device's port by name in its neighbors
//! (`seenUplinks`). Taken over by the resident, nothing happens by itself.
use super::{Named, change::acting_for};
use crate::{
    Error,
    device::{
        interface_live::client_name,
        metrics::{Lease, WlanClient},
        neighbors::{self, SeenNeighbor},
    },
    error_info::ApiError,
    facts::Freshness,
    helper::connect_device::uplink_port,
    topology::{
        DeviceId, TopologyHolder, VlanId,
        access::{AccessTopology, DeviceTarget, device::DeviceAccess, interface::InterfaceAccess},
        findings::InterfaceRef,
    },
};
use async_graphql::{Context, Object, SimpleObject, futures_util::future::join_all};
use ipnet::IpNet;
use std::{collections::BTreeMap, time::Duration};

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
            return Err(Error::NotOwned(format!("VLAN {vlan}").into()).into());
        }
        let vlan = topology
            .list_vlans()
            .find(|other| other.id == VlanId(vlan))
            .ok_or(Error::UnknownVlan(vlan))?;
        // the interfaces that are the VLAN (its VLAN interface, a port with it untagged), with
        // a DHCP server
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
            let leases = servers.contains_key(&device.id());
            let clients = wlans.iter().any(|wlan| wlan.controller.id() == device.id());
            async move { (device.clone(), read(device, leases, clients).await) }
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
    /// Where the routers see `resident`'s RouterOS devices plugged in, where it isn't their
    /// Anschluss in NetBox; each router read once, all at once.
    async fn seen_uplinks(
        &self,
        context: &Context<'_>,
        resident: u32,
    ) -> Result<SeenUplinks, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let mut routers = BTreeMap::<DeviceId, (DeviceAccess, Vec<Linked>)>::new();
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
                .or_insert_with(|| (router, Vec::new()))
                .1
                .push(Linked {
                    device,
                    uplink,
                    router_port,
                });
        }
        let read = join_all(
            routers
                .into_values()
                .map(|(router, linked)| async move { (seen_by(&router).await, router, linked) }),
        )
        .await;
        let mut devices = Vec::new();
        let mut unreachable = Vec::new();
        for (seen, router, linked) in read {
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
        .ok_or(Error::UnknownDevice(router.id().0))?;
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

/// What one device answered.
#[derive(Default)]
struct Tables {
    leases: Box<[Lease]>,
    clients: Box<[WlanClient]>,
}

/// The leases and WLAN clients of `device`, as asked; an error where it doesn't answer.
async fn read(device: &DeviceAccess, leases: bool, clients: bool) -> Result<Tables, Error> {
    // none: no credentials or no way to it, as good as not answering
    let accessible = device
        .connect(DeviceTarget::default())
        .await?
        .ok_or(Error::UnknownDevice(device.id().0))?;
    let freshness = Freshness::MaxAge(MAX_AGE);
    // one after the other: a device's tables are never read at once
    let leases = match leases {
        true => accessible.leases(freshness).await?,
        false => Box::default(),
    };
    let clients = match clients {
        true => accessible.wlan_clients(freshness).await?,
        false => Box::default(),
    };
    Ok(Tables { leases, clients })
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
