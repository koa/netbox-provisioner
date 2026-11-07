//! Scope: Health, resources and PoE out of each device for the map; not the link state (`edges`).
//!
//! Health, resources and PoE out of a device, for the network map's nodes and PoE edges
//! (`docs/concepts/network-map.md`, steps 3 and 4): read over the same way as everything else
//! (`DeviceAccess::connect`), as a fact (`FactKind::DeviceMapData`) per device, so the map's
//! viewers share a reading. Not judged, only shown: the generic rows of `/system/health/print`
//! as `(name, value, unit)`, the few fields of `/system/resource/print` worth a column of their
//! own, and every PoE-capable port (`helper::poe::PoePorts`).

use crate::{
    Error,
    device::raw::{Row, field, rows},
    facts::{self, FactKey, FactKind, Freshness},
    helper::poe::{self, PoePorts},
    topology::{
        DeviceId,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use async_graphql::futures_util::future::join_all;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// How old a reading may be: the map asks every few seconds, but health and resources change
/// slowly, and polling 30-odd devices every few seconds would be about 4 requests a second
/// over the management proxy for no benefit.
pub const DEVICE_DATA_AGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct HealthRow {
    pub name: Box<str>,
    pub value: Box<str>,
    pub unit: Option<Box<str>>,
}

#[derive(Debug, Clone, Default)]
pub struct Resources {
    pub uptime: Box<str>,
    pub cpu_load: u32,
    pub free_memory: u64,
    pub total_memory: u64,
    pub free_hdd_space: u64,
    pub version: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceMapData {
    pub health: Vec<HealthRow>,
    pub resources: Option<Resources>,
    pub poe: PoePorts,
}

/// The health and resources of every device of `devices` that answers, by its id; one that
/// doesn't is left out, same as a device the map leaves off entirely for lack of OSPF.
pub async fn read_all(devices: &[DeviceAccess]) -> BTreeMap<DeviceId, Arc<DeviceMapData>> {
    let reads = devices
        .iter()
        .map(|device| async move { (device.id(), read_one(device).await) });
    join_all(reads)
        .await
        .into_iter()
        .filter_map(|(id, data)| data.map(|data| (id, data)))
        .collect()
}

async fn read_one(device: &DeviceAccess) -> Option<Arc<DeviceMapData>> {
    let key = FactKey::new(FactKind::DeviceMapData, Some(device.id()), "");
    let accessible = match device.connect(DeviceTarget::default()).await {
        Ok(Some(accessible)) => accessible,
        _ => return None,
    };
    let ask = async move {
        let api = accessible.connect_again().await?;
        read(&api).await
    };
    facts::get(key, Freshness::MaxAge(DEVICE_DATA_AGE), ask)
        .await
        .value
        .ok()
}

async fn read(api: &RawDevice<SimpleResult>) -> Result<DeviceMapData, Error> {
    let health = rows(api, "/system/health/print").await?;
    let resources = rows(api, "/system/resource/print").await?;
    let poe = poe::read_ports(api).await?;
    Ok(DeviceMapData {
        poe,
        ..from_rows(&health, &resources)
    })
}

/// The pure part of [`read`]: testable with rows built by hand, no device needed.
fn from_rows(health_rows: &[Row], resource_rows: &[Row]) -> DeviceMapData {
    let health = health_rows
        .iter()
        .filter_map(|row| {
            Some(HealthRow {
                name: field(row, "name")?.into(),
                value: field(row, "value")?.into(),
                unit: field(row, "type").map(Into::into),
            })
        })
        .collect();
    let resources = resource_rows.first().map(|row| Resources {
        uptime: field(row, "uptime").unwrap_or_default().into(),
        cpu_load: field(row, "cpu-load")
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        free_memory: field(row, "free-memory")
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        total_memory: field(row, "total-memory")
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        free_hdd_space: field(row, "free-hdd-space")
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        version: field(row, "version").unwrap_or_default().into(),
    });
    DeviceMapData {
        health,
        resources,
        poe: PoePorts::default(),
    }
}

#[cfg(test)]
mod test;
