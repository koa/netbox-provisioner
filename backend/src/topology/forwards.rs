//! What the NetBox services with tag `forward` open from the internet (`docs/firewall.md`): an
//! IPv4 address gets a port forwarding on every router with `nat` of its site, or where its site
//! has none, on every router with `nat` that announces the default route
//! (`propagate_default_route`) and is reached from the site's routers over OSPF
//! (`ManagementTree::roots_reached`): the answers go out there. An IPv6 one is let through on every
//! router with an interface to the internet. Of two routers with `nat` only one is meant to be
//! in use (a fallback): the answers go out over the default route, so a connection coming in
//! over the other one gets no answer. The generator writes them
//! (`device::ros::firewall`), the findings name what doesn't work out.
use crate::topology::{
    DeviceId, IpAddressId, IpPrefixId, ServiceId, ServicePort, SiteId, Topology, management::ManagementTree,
};
use ipnet::IpNet;
use std::{
    sync::Arc,
    collections::BTreeMap,
    net::{Ipv4Addr, Ipv6Addr},
};

/// A port of an inner IPv4 address, forwarded from the router's uplink (the same port outside).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForward {
    pub service: ServiceId,
    pub address: Ipv4Addr,
    pub port: ServicePort,
}
/// A port of an IPv6 address, open from the internet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    pub service: ServiceId,
    pub address: Ipv6Addr,
    pub port: ServicePort,
}
/// An IPv4 address of a service whose site has no router with `nat`, and no router with `nat`
/// announcing the default route is reached from it: nothing forwards to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrouted {
    pub service: ServiceId,
    pub address: IpAddressId,
    pub site: SiteId,
}
/// A port the router forwards to another service already: this one doesn't get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub service: ServiceId,
    pub port: ServicePort,
    pub router: DeviceId,
    /// the service that has it, the one of the lower NetBox id
    pub by: ServiceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Forwards {
    /// by router, in the order of the services' ids
    pub dstnat: BTreeMap<DeviceId, Vec<PortForward>>,
    /// for every router with an interface to the internet
    pub openings: Vec<Opening>,
    pub unrouted: Vec<Unrouted>,
    /// IPv4 addresses of services (service, address) with no site: neither their prefix, nor
    /// the service's device or virtual machine, nor the device of their interface has one
    pub without_site: Vec<(ServiceId, IpAddressId)>,
    pub taken: Vec<Taken>,
    /// a service with `forward` but no address or no port: it opens nothing
    pub incomplete: Vec<ServiceId>,
}

impl Topology {
    pub fn forwards(self: &Arc<Self>) -> Forwards {
        // the OSPF neighbors, once a site needs them
        let mut tree = None;
        let mut forwards = Forwards::default();
        let mut services = self.services.iter().collect::<Vec<_>>();
        services.sort_by_key(|(id, _)| **id);
        for (&service, data) in services {
            if data.addresses.is_empty() || data.ports.is_empty() {
                forwards.incomplete.push(service);
                continue;
            }
            for &address_id in &data.addresses {
                let Some(address) = self.ip_addresses.get(&address_id) else {
                    continue;
                };
                match address.ip {
                    IpNet::V4(v4) => {
                        let site = self
                            .site_of_prefix(address.prefix)
                            .or_else(|| data.device.and_then(|device| self.devices.get(&device)?.site))
                            .or(data.site)
                            .or_else(|| {
                                let interface = self.interfaces.get(&address.interface?)?;
                                self.devices.get(&interface.device)?.site
                            });
                        let Some(site) = site else {
                            forwards.without_site.push((service, address_id));
                            continue;
                        };
                        let mut routers = self.nat_routers(|data| data.site == Some(site));
                        if routers.is_empty() {
                            let tree = tree.get_or_insert_with(|| ManagementTree::of(self, None));
                            let at_site = self
                                .devices
                                .iter()
                                .filter(|(_, data)| data.site == Some(site))
                                .map(|(id, _)| *id);
                            let reached = tree.roots_reached(at_site);
                            routers = self.nat_routers(|_| true);
                            routers.retain(|router| reached.contains(router));
                        }
                        if routers.is_empty() {
                            forwards.unrouted.push(Unrouted {
                                service,
                                address: address_id,
                                site,
                            });
                        }
                        for router in routers {
                            let on_router = forwards.dstnat.entry(router).or_default();
                            for &port in &data.ports {
                                if let Some(by) = on_router.iter().find(|f| f.port == port) {
                                    // the same service at a second address: the first has it
                                    forwards.taken.push(Taken {
                                        service,
                                        port,
                                        router,
                                        by: by.service,
                                    });
                                    continue;
                                }
                                on_router.push(PortForward {
                                    service,
                                    address: v4.addr(),
                                    port,
                                });
                            }
                        }
                    }
                    IpNet::V6(v6) => forwards.openings.extend(data.ports.iter().map(|&port| Opening {
                        service,
                        address: v6.addr(),
                        port,
                    })),
                }
            }
        }
        forwards.dstnat.retain(|_, forwards| !forwards.is_empty());
        forwards
    }
    /// The site of a prefix, or of the nearest one above it that has one (a customer network is
    /// scoped to its site's prefix).
    fn site_of_prefix(&self, mut prefix: Option<IpPrefixId>) -> Option<SiteId> {
        while let Some(data) = prefix.and_then(|id| self.ip_prefixes.get(&id)) {
            if data.site.is_some() {
                return data.site;
            }
            prefix = data.parent;
        }
        None
    }
    /// The routers with an interface `nat` that `take`, by id.
    fn nat_routers(&self, take: impl Fn(&super::Device) -> bool) -> Vec<DeviceId> {
        let mut routers = self
            .interfaces
            .values()
            .filter(|interface| interface.nat)
            .map(|interface| interface.device)
            .filter(|device| {
                self.devices
                    .get(device)
                    .is_some_and(|data| data.has_routeros && take(data))
            })
            .collect::<Vec<_>>();
        routers.sort();
        routers.dedup();
        routers
    }
}
