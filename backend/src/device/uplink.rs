//! Scope: The quality of a router's uplink, by `/ping` from it to its default gateway
//! (`docs/concepts/monitoring.md`, Z14); not where it shows (`crate::metrics`,
//! `network_state::condition`, `graphql::resident::live`).
//!
//! The gateway is the next hop of the active IPv4 default route of the main table
//! (`immediate-gw`, which a recursive route resolves): no configured target, so the uplink is
//! measured, not the internet behind it.
use super::access::{Row, field, observe};
use crate::{Error, topology::access::device::DeviceAccess};
use async_graphql::SimpleObject;
use ipnet::IpNet;
use mikrotik_model::{
    action::Ping, command::Reads, model::IpRoute, resource::collect_resource, value::IpOrInterface,
};
use std::{net::IpAddr, time::Duration};

/// Whether `device` has an uplink: a port tagged `internet`, or it announces the default route
/// (`propagate_default_route`), which it has from an uplink NetBox doesn't tag.
pub fn has_uplink(device: &DeviceAccess) -> bool {
    device.propagates_default_route() || device.interfaces().iter().any(|i| i.is_internet())
}

/// Pings of one measurement: a second together.
const COUNT: u8 = 5;
const INTERVAL: Duration = Duration::from_millis(200);

/// What the pings to the uplink's gateway got.
#[derive(Debug, Clone, PartialEq)]
pub struct UplinkPing {
    pub gateway: IpAddr,
    pub sent: u32,
    pub received: u32,
    /// the average round trip of the answered ones; none without an answer
    pub rtt: Option<Duration>,
}

/// [`UplinkPing`] as the pages show it.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct UplinkQuality {
    pub gateway: Box<str>,
    /// the share lost, 0 to 1
    pub loss: f64,
    /// the average round trip of the answered ones, milliseconds
    pub rtt_ms: Option<f64>,
}

impl From<&UplinkPing> for UplinkQuality {
    fn from(ping: &UplinkPing) -> Self {
        UplinkQuality {
            gateway: ping.gateway.to_string().into(),
            loss: ping.loss(),
            rtt_ms: ping.rtt.map(|rtt| rtt.as_secs_f64() * 1000.0),
        }
    }
}

impl UplinkPing {
    /// The share of the pings lost, 0 to 1.
    pub fn loss(&self) -> f64 {
        match self.sent {
            0 => 1.0,
            sent => f64::from(sent.saturating_sub(self.received)) / f64::from(sent),
        }
    }
}

/// What a router's uplink reading found.
#[derive(Debug, Clone, PartialEq)]
pub enum UplinkReading {
    /// no active IPv4 default route in the main table: no internet
    NoDefaultRoute,
    /// the default route goes out an interface without a next hop's address (PPPoE, LTE):
    /// nothing to ping, the uplink unmeasured
    NoAddress {
        interface: Box<str>,
    },
    Pinged(UplinkPing),
}

/// The next hop of the default route.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NextHop {
    Address(IpAddr),
    Interface(Box<str>),
}

/// Reads the device's default route and pings its next hop's address.
pub async fn ping(device: &impl Reads) -> Result<UplinkReading, Error> {
    let routes = collect_resource::<IpRoute>(device).await?;
    let gateway = match default_gateway(&routes) {
        None => return Ok(UplinkReading::NoDefaultRoute),
        Some(NextHop::Interface(interface)) => return Ok(UplinkReading::NoAddress { interface }),
        Some(NextHop::Address(gateway)) => gateway,
    };
    let ping = Ping {
        address: gateway,
        count: COUNT,
        interval: INTERVAL,
    };
    Ok(UplinkReading::Pinged(summary(
        gateway,
        &observe(device, &ping).await?,
    )))
}

/// The next hop of the active IPv4 default route of the main table.
pub(crate) fn default_gateway(routes: &[IpRoute]) -> Option<NextHop> {
    routes
        .iter()
        .filter(|route| {
            route.status.active == Some(true)
                && matches!(route.cfg.dst_address, IpNet::V4(net) if net.prefix_len() == 0)
                && route
                    .cfg
                    .routing_table
                    .as_ref()
                    .is_none_or(|table| &table[..] == b"main")
        })
        .map(|route| {
            let hop = route
                .status
                .immediate_gw
                .as_ref()
                .unwrap_or(&route.cfg.gateway);
            match hop {
                IpOrInterface::Ip(ip) => NextHop::Address(*ip),
                IpOrInterface::IpWithInterface(ip) => NextHop::Address(ip.ip),
                IpOrInterface::Interface(interface) => match &route.cfg.gateway {
                    IpOrInterface::Ip(ip) => NextHop::Address(*ip),
                    _ => NextHop::Interface(String::from_utf8_lossy(interface).into()),
                },
            }
        })
        .next()
}

/// The last row of a ping counts them all (`sent`, `received`, `avg-rtt`).
pub(crate) fn summary(gateway: IpAddr, rows: &[Row]) -> UplinkPing {
    let last = rows.last();
    let count = |name| {
        last.and_then(|row| field(row, name))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    let received = count("received");
    UplinkPing {
        gateway,
        sent: count("sent"),
        received,
        rtt: (received > 0)
            .then(|| last.and_then(|row| field(row, "avg-rtt")).and_then(rtt))
            .flatten(),
    }
}

/// A unit of a round trip, in microseconds.
struct Unit {
    name: &'static str,
    micros: u64,
}

/// `ms` before `s`, which it starts with.
const UNITS: &[Unit] = &[
    Unit {
        name: "ms",
        micros: 1_000,
    },
    Unit {
        name: "us",
        micros: 1,
    },
    Unit {
        name: "s",
        micros: 1_000_000,
    },
];

/// A round trip as RouterOS gives it: `1ms234us`, `342us`, `12ms`.
fn rtt(value: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut rest = value;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let number: u64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit = UNITS.iter().find(|unit| rest.starts_with(unit.name))?;
        total += Duration::from_micros(number * unit.micros);
        rest = &rest[unit.name.len()..];
    }
    Some(total)
}

#[cfg(test)]
mod test;
