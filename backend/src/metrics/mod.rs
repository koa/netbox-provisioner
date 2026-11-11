//! Scope: The devices' metrics at `/metrics`: the collector reading every device in turn and
//! the last round it keeps; not the reading itself (`device::metrics`,
//! `network_state::device_data`) nor the provisioner's own metrics (`facts`, HTTP).
//!
//! What the mikrotik-metrik-exporter served (C14, `docs/device-metrics.md`), with `device` and
//! `site` from NetBox: every `device_metrics_seconds` (default 60, `0` off) the collector reads
//! every device with RouterOS and credentials, all at once, each within
//! [`DEVICE_TIMEOUT`], and keeps the round; a scrape renders the last one
//! ([`render::families`]). It only reads, as the network map does, and shares that reading
//! (health, resources, PoE).

mod render;
#[cfg(test)]
mod test;

pub(crate) use render::{DeviceSample, Snapshot};

use crate::{
    config::CONFIG,
    lock::LockAnyway,
    network_state::device_data,
    topology::{
        TopologyHolder,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use async_graphql::futures_util::future::join_all;
use log::warn;
use prometheus::{
    Registry,
    core::{Collector, Desc},
    proto::MetricFamily,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// How long one device may take in a round; one that takes longer counts as down.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(30);

/// The last round, rendered at each scrape.
static LAST: Mutex<Option<Arc<Snapshot>>> = Mutex::new(None);

/// Reads every device once per `device_metrics_seconds`, a round not before the last one ended.
pub fn start(topology: TopologyHolder) {
    let Some(every) = CONFIG.device_metrics_every() else {
        return;
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let devices = topology.devices().await;
            let snapshot = round(&devices).await;
            *LAST.lock_anyway() = Some(Arc::new(snapshot));
        }
    });
}

/// Puts the devices' metrics into `registry` (`/metrics`).
pub fn register(registry: &Registry) {
    if let Err(error) = registry.register(Box::new(DevicesCollector)) {
        warn!("cannot register the device metrics: {error}");
    }
}

/// One reading of every device that has RouterOS and credentials.
async fn round(devices: &[DeviceAccess]) -> Snapshot {
    let reads = devices
        .iter()
        .filter(|device| device.has_routeros() && device.credentials().is_some())
        .map(|device| async move {
            tokio::time::timeout(DEVICE_TIMEOUT, sample(device))
                .await
                .unwrap_or_else(|_| DeviceSample::down(device))
        });
    Snapshot {
        devices: join_all(reads).await.into(),
    }
}

/// What `device` answers; down where it can't be reached or doesn't answer the reading.
async fn sample(device: &DeviceAccess) -> DeviceSample {
    let accessible = match device.connect(DeviceTarget::default()).await {
        Ok(Some(accessible)) => accessible,
        _ => return DeviceSample::down(device),
    };
    let metrics = accessible.metrics().await.value.ok();
    let map = device_data::read_one(device).await;
    DeviceSample {
        up: metrics.is_some(),
        map,
        metrics,
        ..DeviceSample::down(device)
    }
}

/// Renders the last round at each scrape. Its metrics change with the devices, so it has no
/// fixed descriptors (an unchecked collector).
struct DevicesCollector;

impl Collector for DevicesCollector {
    fn desc(&self) -> Vec<&Desc> {
        Vec::new()
    }
    fn collect(&self) -> Vec<MetricFamily> {
        let last = LAST.lock_anyway().clone();
        last.map(|snapshot| render::families(&snapshot))
            .unwrap_or_default()
    }
}
