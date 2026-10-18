//! The way from each RouterOS device to the management roots (`docs/concepts/watchdog.md`):
//! the devices whose OSPF announces the default route, and the management proxy's device.
//! Two devices are neighbors where both have an `ospf` interface with an IPv4 address in the
//! same network; each device's parent is its neighbor nearest to a root (by hops, then one it
//! is cabled to, then by NetBox's order of the names). The ping watchdog of a device watches its parent.
use crate::topology::{
    DeviceId, Topology,
    access::{device::DeviceAccess, interface::InterfaceAccess},
    natural::naturalize,
};
use ipnet::IpNet;
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::Arc,
};

/// The parent of every RouterOS device a root is reached from.
#[derive(Debug, Default)]
pub struct ManagementTree {
    parents: HashMap<DeviceId, DeviceId>,
    roots: BTreeSet<DeviceId>,
    networks: HashMap<DeviceId, BTreeSet<IpNet>>,
}

impl ManagementTree {
    /// The tree towards the devices with `propagate_default_route` and the device named
    /// `proxy_device` (the management proxy's), if any.
    pub fn of(topology: &Arc<Topology>, proxy_device: Option<&str>) -> ManagementTree {
        let mut devices = topology
            .list_devices()
            .filter(DeviceAccess::has_routeros)
            .collect::<Vec<_>>();
        devices.sort_by_cached_key(|device| naturalize(device.name()));
        let networks = devices
            .iter()
            .map(|device| (device.id(), ospf_networks(device)))
            .collect::<HashMap<_, _>>();
        let roots = devices
            .iter()
            .filter(|device| {
                device.propagates_default_route() || proxy_device == Some(device.name())
            })
            .map(DeviceAccess::id)
            .collect::<BTreeSet<_>>();
        // hops from the nearest root, then the parent: of the neighbors one hop nearer, one
        // it is cabled to (not one across a radio bridge's segment), then by name
        let mut depth = roots.iter().map(|root| (*root, 0)).collect::<HashMap<_, _>>();
        let mut todo = devices
            .iter()
            .filter(|device| roots.contains(&device.id()))
            .map(DeviceAccess::id)
            .collect::<VecDeque<_>>();
        while let Some(near) = todo.pop_front() {
            for far in &devices {
                let far = far.id();
                if !depth.contains_key(&far) && !networks[&near].is_disjoint(&networks[&far]) {
                    depth.insert(far, depth[&near] + 1);
                    todo.push_back(far);
                }
            }
        }
        let mut parents = HashMap::new();
        for device in &devices {
            let Some(hops) = depth.get(&device.id()).filter(|hops| **hops > 0) else {
                continue;
            };
            let cabled = cabled_to(device);
            let parent = devices
                .iter()
                .filter(|near| depth.get(&near.id()) == Some(&(hops - 1)))
                .filter(|near| !networks[&near.id()].is_disjoint(&networks[&device.id()]))
                .min_by_key(|near| !cabled.contains(&near.id()));
            if let Some(parent) = parent {
                parents.insert(device.id(), parent.id());
            }
        }
        ManagementTree {
            parents,
            roots,
            networks,
        }
    }

    /// The neighbor of `device` on its way to a root; `None` for a root and a device no root is
    /// reached from.
    pub fn parent(&self, device: DeviceId) -> Option<DeviceId> {
        self.parents.get(&device).copied()
    }

    pub fn is_root(&self, device: DeviceId) -> bool {
        self.roots.contains(&device)
    }

    /// The devices whose parent `device` is.
    pub fn children(&self, device: DeviceId) -> impl Iterator<Item = DeviceId> + '_ {
        self.parents
            .iter()
            .filter(move |(_, parent)| **parent == device)
            .map(|(child, _)| *child)
    }

    /// The devices that may lose their parent while `device` is away: its children, and those
    /// reaching their parent in a network `device` is in too (the station of a radio bridge
    /// reaches the near router through the AP, all in the bridge's segment).
    pub fn depending_on(&self, device: DeviceId) -> BTreeSet<DeviceId> {
        let none = BTreeSet::new();
        let networks = |id: &DeviceId| self.networks.get(id).unwrap_or(&none);
        let own = networks(&device);
        self.parents
            .iter()
            .filter(|(child, parent)| {
                **parent == device
                    || (**child != device
                        && networks(child)
                            .intersection(networks(parent))
                            .any(|net| own.contains(net)))
            })
            .map(|(child, _)| *child)
            .collect()
    }
}

/// The devices `device` has a cable to.
fn cabled_to(device: &DeviceAccess) -> BTreeSet<DeviceId> {
    device
        .interfaces()
        .iter()
        .flat_map(|port| port.connected_interfaces())
        .filter_map(|far| far.device())
        .map(|far| far.id())
        .collect()
}

/// The `ospf` interfaces of `device` in a network with `other`: where it reaches it.
pub fn shared_interfaces(device: &DeviceAccess, other: &DeviceAccess) -> Vec<InterfaceAccess> {
    let theirs = ospf_networks(other);
    device
        .interfaces()
        .iter()
        .filter(|interface| interface.use_ospf())
        .filter(|interface| !networks_of(interface).is_disjoint(&theirs))
        .cloned()
        .collect()
}

/// The IPv4 networks of the device's `ospf` interfaces (a `/32` on `lo` is no network).
fn ospf_networks(device: &DeviceAccess) -> BTreeSet<IpNet> {
    device
        .interfaces()
        .iter()
        .filter(|interface| interface.use_ospf())
        .flat_map(networks_of)
        .collect()
}

fn networks_of(interface: &InterfaceAccess) -> BTreeSet<IpNet> {
    interface
        .ips()
        .iter()
        .filter_map(|ip| ip.net())
        .filter(|net| matches!(net, IpNet::V4(net) if net.prefix_len() < 32))
        .map(|net| net.trunc())
        .collect()
}
