//! What the NetBox services with tag `forward` open from the internet (`docs/firewall.md`): an
//! IPv4 address gets a port forwarding on the router with `nat` of its site, an IPv6 one is let
//! through on every router with an interface to the internet. The generator writes them
//! (`device::ros::firewall`), the findings name what doesn't work out.
use crate::topology::{DeviceId, IpAddressId, IpPrefixId, ServiceId, ServicePort, SiteId, Topology};
use ipnet::IpNet;
use std::{
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
/// An IPv4 address of a service whose site has no router with `nat`, or several: nothing
/// forwards to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrouted {
    pub service: ServiceId,
    pub address: IpAddressId,
    /// the routers with `nat` of its site
    pub routers: Box<[DeviceId]>,
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
    pub taken: Vec<Taken>,
    /// a service with `forward` but no address or no port: it opens nothing
    pub incomplete: Vec<ServiceId>,
}

impl Topology {
    pub fn forwards(&self) -> Forwards {
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
                            .or_else(|| {
                                let interface = self.interfaces.get(&address.interface?)?;
                                self.devices.get(&interface.device)?.site
                            });
                        let routers = site.map(|site| self.nat_routers(site)).unwrap_or_default();
                        let [router] = routers[..] else {
                            forwards.unrouted.push(Unrouted {
                                service,
                                address: address_id,
                                routers: routers.into_boxed_slice(),
                            });
                            continue;
                        };
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
    /// The routers of `site` with an interface `nat`, by id.
    fn nat_routers(&self, site: SiteId) -> Vec<DeviceId> {
        let mut routers = self
            .interfaces
            .values()
            .filter(|interface| interface.nat)
            .map(|interface| interface.device)
            .filter(|device| {
                self.devices
                    .get(device)
                    .is_some_and(|data| data.has_routeros && data.site == Some(site))
            })
            .collect::<Vec<_>>();
        routers.sort();
        routers.dedup();
        routers
    }
}
