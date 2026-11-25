//! Scope: The devices' metrics at `/metrics`: each scrape reading every device and rendering
//! that with the registry's own metrics; not the reading itself (`device::metrics`,
//! `network_state::device_data`) nor the provisioner's own metrics (`facts`, HTTP).
//!
//! What the mikrotik-metrik-exporter served (C14, `docs/device-metrics.md`), with `device` and
//! `site` from NetBox: each scrape reads every device with RouterOS and credentials, all at
//! once, each within [`DEVICE_TIMEOUT`] (`device_metrics: false` reads none). The readings are
//! facts begun after the scrape began (`Freshness::Since`): nothing older is served, and a
//! reading of the same device and tables begun since (a second scrape, the network map) is
//! joined, so each scrape asks each device once. It only reads, as the network map does, and
//! shares that reading (health, resources, PoE).

mod render;
#[cfg(test)]
mod test;

pub(crate) use render::{DeviceSample, LinkSample, Snapshot};

use crate::{
    config::CONFIG,
    device::uplink,
    facts::Freshness,
    network_state::{
        device_data,
        edges::{self, EdgeKind, EdgeState},
        read,
    },
    topology::{
        TopologyHolder,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use async_graphql::futures_util::future::join_all;
use prometheus::{Encoder, Registry, TextEncoder};
use std::time::Duration;
use tokio::time::Instant;

/// How long the devices may take in a scrape at most; one that takes longer counts as down.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(20);

/// What a scrape keeps of the time Prometheus gives it for rendering and sending.
const SCRAPE_MARGIN: Duration = Duration::from_secs(1);

/// The text of `/metrics`: `registry`'s metrics and every device's, read now. `scrape_timeout`
/// is how long Prometheus waits (`X-Prometheus-Scrape-Timeout-Seconds`): the devices not done
/// [`SCRAPE_MARGIN`] before count as down, so one slow device doesn't fail the whole scrape
/// (Prometheus would take all its series as gone).
pub async fn scrape(
    registry: &Registry,
    topology: &TopologyHolder,
    scrape_timeout: Option<Duration>,
) -> prometheus::Result<String> {
    let since = Instant::now();
    let mut families = registry.gather();
    if CONFIG.device_metrics() {
        let budget = device_budget(scrape_timeout);
        let devices = topology.devices().await;
        let deadline = since + budget;
        let (round, links) = tokio::join!(
            round(&devices, since, deadline),
            tokio::time::timeout_at(deadline, links(&devices))
        );
        families.extend(render::families(&Snapshot {
            links: links.unwrap_or_default(),
            ..round
        }));
    }
    let mut text = Vec::new();
    TextEncoder::new().encode(&families, &mut text)?;
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// How long the devices may take in a scrape Prometheus waits `scrape_timeout` for.
pub(crate) fn device_budget(scrape_timeout: Option<Duration>) -> Duration {
    scrape_timeout
        .map(|timeout| timeout.saturating_sub(SCRAPE_MARGIN))
        .map_or(DEVICE_TIMEOUT, |budget| budget.min(DEVICE_TIMEOUT))
}

/// One reading of every device that has RouterOS and credentials, begun after `since`; a
/// device not done by `deadline` is down.
async fn round(devices: &[DeviceAccess], since: Instant, deadline: Instant) -> Snapshot {
    let reads = devices
        .iter()
        .filter(|device| device.has_routeros() && device.credentials().is_some())
        .map(|device| async move {
            tokio::time::timeout_at(deadline, sample(device, since))
                .await
                .unwrap_or_else(|_| DeviceSample::down(device))
        });
    Snapshot {
        devices: join_all(reads).await.into(),
        links: Box::default(),
    }
}

/// NetBox's cables and wireless links between OSPF routers with their state in the link-state
/// database of the first router that answers (as the network map reads it); none if none does.
async fn links(devices: &[DeviceAccess]) -> Box<[LinkSample]> {
    let Ok(Some(read)) = read::read(&read::readers(devices)).await else {
        return Box::default();
    };
    let topology = read.device.topology().clone();
    edges::edges(devices, &read.lsdb)
        .into_iter()
        .filter_map(|edge| {
            let (up, ipv6_up) = match edge.state {
                EdgeState::Up => (true, true),
                EdgeState::V3Down => (true, false),
                EdgeState::Down(_) => (false, false),
                EdgeState::NotOspf => return None,
            };
            let a = topology.get_interface_by_id(edge.a)?;
            let b = topology.get_interface_by_id(edge.b)?;
            let (device, peer) = (a.device()?, b.device()?);
            Some(LinkSample {
                site: device
                    .site()
                    .map(|site| site.name().into())
                    .unwrap_or_default(),
                device: device.name().into(),
                interface: a.name().into(),
                peer_device: peer.name().into(),
                peer_interface: b.name().into(),
                kind: match edge.kind {
                    EdgeKind::Cable => "cable",
                    EdgeKind::Radio => "radio",
                },
                up,
                ipv6_up,
            })
        })
        .collect()
}

/// What `device` answers, over one login; down where it can't be reached or doesn't answer
/// the reading.
async fn sample(device: &DeviceAccess, since: Instant) -> DeviceSample {
    let accessible = match device.connect(DeviceTarget::default()).await {
        Ok(Some(accessible)) => accessible,
        _ => return DeviceSample::down(device),
    };
    let freshness = Freshness::Since(since);
    let metrics = accessible.metrics(freshness).await.ok();
    let map = device_data::read_accessible(&accessible, freshness).await;
    // a router with an uplink; after the readings, one device's reads stay sequential
    let uplink = match uplink::has_uplink(device) && metrics.is_some() {
        true => uplink::ping(&accessible.reader().at(freshness))
            .await
            .ok()
            .flatten(),
        false => None,
    };
    DeviceSample {
        up: metrics.is_some(),
        map,
        metrics,
        uplink,
        ..DeviceSample::down(device)
    }
}
