//! Scope: The quality of a router's uplink, by `/ping` from it to its default gateway
//! (`docs/concepts/monitoring.md`, Z14); not where it shows (`crate::metrics`).
//!
//! The gateway is the next hop of the active IPv4 default route of the main table
//! (`immediate-gw`, which a recursive route resolves): no configured target, so the uplink is
//! measured, not the internet behind it.
use super::access::{Row, field, observe};
use crate::Error;
use ipnet::IpNet;
use mikrotik_model::{
    action::Ping, command::Reads, model::IpRoute, resource::collect_resource, value::IpOrInterface,
};
use std::{net::IpAddr, time::Duration};

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

impl UplinkPing {
    /// The share of the pings lost, 0 to 1.
    pub fn loss(&self) -> f64 {
        match self.sent {
            0 => 1.0,
            sent => f64::from(sent.saturating_sub(self.received)) / f64::from(sent),
        }
    }
}

/// Pings the device's default gateway; none without an active default route to an address.
pub async fn ping(device: &impl Reads) -> Result<Option<UplinkPing>, Error> {
    let routes = collect_resource::<IpRoute>(device).await?;
    let Some(gateway) = default_gateway(&routes) else {
        return Ok(None);
    };
    let ping = Ping {
        address: gateway,
        count: COUNT,
        interval: INTERVAL,
    };
    Ok(Some(summary(gateway, &observe(device, &ping).await?)))
}

/// The next hop of the active IPv4 default route of the main table.
pub(crate) fn default_gateway(routes: &[IpRoute]) -> Option<IpAddr> {
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
        .find_map(|route| {
            let hop = route
                .status
                .immediate_gw
                .as_ref()
                .unwrap_or(&route.cfg.gateway);
            match hop {
                IpOrInterface::Ip(ip) => Some(*ip),
                IpOrInterface::IpWithInterface(ip) => Some(ip.ip),
                IpOrInterface::Interface(_) => match &route.cfg.gateway {
                    IpOrInterface::Ip(ip) => Some(*ip),
                    _ => None,
                },
            }
        })
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

/// A round trip as RouterOS gives it: `1ms234us`, `342us`, `12ms`.
fn rtt(value: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut rest = value;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let number: u64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let (unit, after) = [("ms", 1_000), ("us", 1), ("s", 1_000_000)]
            .into_iter()
            .find_map(|(unit, micros)| rest.strip_prefix(unit).map(|after| (micros, after)))?;
        total += Duration::from_micros(number * unit);
        rest = after;
    }
    Some(total)
}

#[cfg(test)]
mod test;
