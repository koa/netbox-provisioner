//! Scope: The devices' metrics at `/metrics`: each scrape reading every device and rendering
//! that with the registry's own metrics; not the reading itself (`device::metrics`,
//! `network_state::device_data`) nor the provisioner's own metrics (`facts`, HTTP).
//!
//! What the mikrotik-metrik-exporter served (C14, `docs/device-metrics.md`), with `device` and
//! `site` from NetBox: each scrape reads every device with RouterOS and credentials, all at
//! once, each within [`DEVICE_TIMEOUT`] (`device_metrics: false` reads none). The readings are
//! facts begun after the scrape began (`Freshness::Since`): nothing older is served, and a
//! reading of the same device and menus begun since (a second scrape, the network map) is
//! joined, so each scrape asks each device once. It only reads, as the network map does, and
//! shares that reading (health, resources, PoE).

mod render;
#[cfg(test)]
mod test;

pub(crate) use render::{DeviceSample, Snapshot};

use crate::{
    config::CONFIG,
    facts::Freshness,
    network_state::device_data,
    topology::{
        TopologyHolder,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use async_graphql::futures_util::future::join_all;
use prometheus::{Encoder, Registry, TextEncoder};
use std::time::Duration;
use tokio::time::Instant;

/// How long one device may take in a scrape; one that takes longer counts as down. Below the
/// chart's `serviceMonitor.scrapeTimeout`, or Prometheus gives up on the whole scrape first.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(20);

/// The text of `/metrics`: `registry`'s metrics and every device's, read now.
pub async fn scrape(registry: &Registry, topology: &TopologyHolder) -> prometheus::Result<String> {
    let mut families = registry.gather();
    if CONFIG.device_metrics() {
        let since = Instant::now();
        let devices = topology.devices().await;
        families.extend(render::families(&round(&devices, since).await));
    }
    let mut text = Vec::new();
    TextEncoder::new().encode(&families, &mut text)?;
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// One reading of every device that has RouterOS and credentials, begun after `since`.
async fn round(devices: &[DeviceAccess], since: Instant) -> Snapshot {
    let reads = devices
        .iter()
        .filter(|device| device.has_routeros() && device.credentials().is_some())
        .map(|device| async move {
            tokio::time::timeout(DEVICE_TIMEOUT, sample(device, since))
                .await
                .unwrap_or_else(|_| DeviceSample::down(device))
        });
    Snapshot {
        devices: join_all(reads).await.into(),
    }
}

/// What `device` answers, over one login; down where it can't be reached or doesn't answer
/// the reading.
async fn sample(device: &DeviceAccess, since: Instant) -> DeviceSample {
    let accessible = match device.connect(DeviceTarget::default()).await {
        Ok(Some(accessible)) => accessible,
        _ => return DeviceSample::down(device),
    };
    let freshness = Freshness::Since(since);
    let metrics = accessible.metrics(freshness).await.value.ok();
    let map = device_data::read_accessible(device, &accessible, freshness).await;
    DeviceSample {
        up: metrics.is_some(),
        map,
        metrics,
        ..DeviceSample::down(device)
    }
}
