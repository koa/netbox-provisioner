//! What the network routes, against what NetBox documents: an address free in NetBox may be in
//! use by a device NetBox doesn't know of, which OSPF announces. Read from one router of the
//! OSPF area (`Routes::read`), which knows the routes of all. Read only.
use crate::{
    Error,
    config::CONFIG,
    device::{
        Credentials, Path, bootstrap::reachable_address, connect,
        raw::{command, field},
    },
    topology::{Topology, access::device::DeviceAccess},
};
use async_graphql::SimpleObject;
use ipnet::IpNet;
use mikrotik_api::simple::SimpleResult;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::time::timeout;

/// The routes of one router, IPv4 and IPv6.
pub struct Routes {
    pub router: Box<str>,
    routes: Vec<Route>,
}

struct Route {
    dst: IpNet,
    gateway: Option<Box<str>>,
}

/// An address NetBox has free that the network routes: in use, but not documented.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct UndocumentedAddress {
    pub address: Box<str>,
    /// the route it is in, `192.168.100.9/32`
    pub route: Box<str>,
    pub gateway: Option<Box<str>>,
    /// the router that has the route
    pub router: Box<str>,
}

impl Routes {
    /// Reads the routes of a router of the OSPF area, which all know the same: the management
    /// proxy's device if there is one, else the routers with the most OSPF interfaces (the
    /// core), the first of [`TRIED`] that answers. An error when none does, as an address
    /// can't be handed out unchecked.
    pub async fn read(topology: &Arc<Topology>) -> Result<Routes, Error> {
        let mut failures = Vec::new();
        let proxy = CONFIG.management_proxy.as_ref().map(|proxy| proxy.device.as_ref());
        for router in routers(topology, proxy).into_iter().take(TRIED) {
            match timeout(TIMEOUT, read_from(&router)).await {
                Ok(Ok(routes)) => {
                    return Ok(Routes {
                        router: router.name().into(),
                        routes,
                    });
                }
                Ok(Err(error)) => failures.push(format!("{}: {error}", router.name())),
                Err(_) => failures.push(format!("{}: no answer", router.name())),
            }
        }
        Err(Error::NoRouteCheck(if failures.is_empty() {
            "no router with OSPF and an address in NetBox".into()
        } else {
            failures.join("; ").into()
        }))
    }

    /// The route that shows `host` of the pool `pool` in use: one more specific than the pool
    /// (the pool itself, a summary or the default route say nothing about one address).
    pub fn in_use(&self, host: IpAddr, pool: IpNet) -> Option<UndocumentedAddress> {
        self.routes
            .iter()
            .filter(|route| route.dst.contains(&host) && route.dst.prefix_len() > pool.prefix_len())
            .max_by_key(|route| route.dst.prefix_len())
            .map(|route| UndocumentedAddress {
                address: host.to_string().into(),
                route: route.dst.to_string().into(),
                gateway: route.gateway.clone(),
                router: self.router.clone(),
            })
    }
}

/// How many routers are asked before giving up, and how long each may take.
const TRIED: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The routers to ask, in order: the management proxy's device, then the RouterOS devices with
/// OSPF interfaces and an address, the most OSPF interfaces first.
fn routers(topology: &Arc<Topology>, proxy: Option<&str>) -> Vec<DeviceAccess> {
    let mut routers = topology
        .list_devices()
        .filter(|device| device.has_routeros() && device.primary_ip().is_some())
        .map(|device| {
            let ospf = device.interfaces().iter().filter(|i| i.use_ospf()).count();
            (device, ospf)
        })
        .filter(|(device, ospf)| *ospf > 0 || Some(device.name()) == proxy)
        .collect::<Vec<_>>();
    routers.sort_by_key(|(device, ospf)| {
        (Some(device.name()) != proxy, std::cmp::Reverse(*ospf), device.name().to_string())
    });
    routers.into_iter().map(|(device, _)| device).collect()
}

async fn read_from(router: &DeviceAccess) -> Result<Vec<Route>, Error> {
    let address = reachable_address(router).ok_or_else(|| Error::NoAddress(router.name().into()))?;
    let credentials =
        Credentials::Named(router.credentials().ok_or(Error::MissingCredentials)?.into());
    let client = connect::<SimpleResult>(address, &Path::Routed, &credentials).await?;
    let mut routes = Vec::new();
    for menu in ["/ip/route/print", "/ipv6/route/print"] {
        for row in command(&client, menu, &[(".proplist", "dst-address,gateway")]).await? {
            if let Some(dst) = field(&row, "dst-address").and_then(|dst| dst.parse().ok()) {
                routes.push(Route {
                    dst,
                    gateway: field(&row, "gateway").map(Box::from),
                });
            }
        }
    }
    Ok(routes)
}

#[cfg(test)]
mod test {
    use super::*;

    fn routes(routes: &[&str]) -> Routes {
        Routes {
            router: "core".into(),
            routes: routes
                .iter()
                .map(|dst| Route {
                    dst: dst.parse().unwrap(),
                    gateway: Some("ether1".into()),
                })
                .collect(),
        }
    }

    #[test]
    fn an_address_in_a_more_specific_route_is_in_use() {
        let pool: IpNet = "192.168.100.0/26".parse().unwrap();
        let routes = routes(&["0.0.0.0/0", "192.168.100.0/24", "192.168.100.0/26", "192.168.100.9/32"]);
        let taken = routes.in_use("192.168.100.9".parse().unwrap(), pool);
        assert_eq!(taken.map(|t| t.route), Some("192.168.100.9/32".into()));
        assert_eq!(routes.in_use("192.168.100.10".parse().unwrap(), pool), None);
    }

    /// Without a management proxy the core routers are asked, the proxy's device first.
    #[test]
    fn the_routers_with_the_most_ospf_first() {
        let topology = crate::device::ros::netbox_example_test::example_topology();
        let names = |proxy| {
            routers(&topology, proxy)
                .iter()
                .take(TRIED)
                .map(|d| d.name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(None), ["srv-router-berg", "switch-mimi", "klr-sw-1"]);
        assert_eq!(names(Some("wrk-sw1"))[0], "wrk-sw1");
    }

    #[test]
    fn a_link_network_over_the_address_is_in_use() {
        let pool: IpNet = "2a02:16a:fe00:8000::/64".parse().unwrap();
        let routes = routes(&["2a02:16a:fe00:8000::8/126"]);
        assert!(routes.in_use("2a02:16a:fe00:8000::9".parse().unwrap(), pool).is_some());
    }
}
