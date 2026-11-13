//! Scope: Health, resources and PoE out of each device for the map; not the link state (`edges`).
//!
//! Health, resources and PoE out of a device, for the network map's nodes and PoE edges
//! (`docs/network-map.md`, steps 3 and 4): read over the same way as everything else
//! (`DeviceAccess::connect`), each menu as a fact (`access::Reader::at`), so the map's viewers
//! share a reading. Not judged, only shown: the rows of `/system/health/print` as name,
//! value and unit, the few fields of `/system/resource/print` worth a column of their own, and
//! every PoE-capable port (`device::poe::PoePorts`).

use crate::{
    Error,
    device::AccessibleDevice,
    device::poe::{self, PoeLoad, PoePorts},
    facts::Freshness,
    topology::{
        DeviceId,
        access::{DeviceTarget, device::DeviceAccess},
    },
};
use async_graphql::futures_util::future::join_all;
use mikrotik_model::{
    command::Reads,
    model::{SystemHealthState, SystemResourceState, SystemRouterboardState},
    resource::{SingleResource, collect_resource},
};
use std::{collections::BTreeMap, time::Duration};

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
    pub uptime: Duration,
    pub cpu_load: u32,
    pub free_memory: u64,
    pub total_memory: u64,
    pub free_hdd_space: u64,
    pub total_hdd_space: u64,
    pub version: Box<str>,
    /// `board-name`, as `/system/resource` reports it (`hAP ax^2`)
    pub board_name: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceMapData {
    pub health: Box<[HealthRow]>,
    pub resources: Option<Resources>,
    pub poe: PoePorts,
    /// what its PoE out groups draw against the board's limits (`device::poe::load`)
    pub poe_load: Box<[PoeLoad]>,
}

/// The health and resources of every device of `devices` that answers, by its id; one that
/// doesn't is left out, same as a device the map leaves off entirely for lack of OSPF.
pub async fn read_all(devices: &[DeviceAccess]) -> BTreeMap<DeviceId, DeviceMapData> {
    let reads = devices
        .iter()
        .map(|device| async move { (device.id(), read_one(device).await) });
    join_all(reads)
        .await
        .into_iter()
        .filter_map(|(id, data)| data.map(|data| (id, data)))
        .collect()
}

/// The reading of `device`, at most [`DEVICE_DATA_AGE`] old; `None` where it isn't reached
/// or doesn't answer.
pub(crate) async fn read_one(device: &DeviceAccess) -> Option<DeviceMapData> {
    let accessible = match device.connect(DeviceTarget::default()).await {
        Ok(Some(accessible)) => accessible,
        _ => return None,
    };
    read_accessible(&accessible, Freshness::MaxAge(DEVICE_DATA_AGE)).await
}

/// The reading of `device`, logged in as `accessible`, as fresh as `freshness` asks; shared
/// with `/metrics` (`crate::metrics`).
pub(crate) async fn read_accessible(
    accessible: &AccessibleDevice,
    freshness: Freshness,
) -> Option<DeviceMapData> {
    read(&accessible.reader().at(freshness)).await.ok()
}

async fn read(api: &impl Reads) -> Result<DeviceMapData, Error> {
    let health = collect_resource::<SystemHealthState>(api).await?;
    let resources = SystemResourceState::fetch(api).await?;
    let poe = poe::read_ports(api).await?;
    // the board's limits; read only where a port feeds
    let poe_load = if poe.ethernet.iter().any(|port| port.poe_out) {
        match SystemRouterboardState::fetch(api).await? {
            Some(board) => poe::load(&board.model.0, &poe).into(),
            None => Box::default(),
        }
    } else {
        Box::default()
    };
    Ok(DeviceMapData {
        poe,
        poe_load,
        ..from_states(&health, resources.as_ref())
    })
}

/// The pure part of [`read`]: testable with rows built by hand, no device needed.
fn from_states(
    health: &[SystemHealthState],
    resources: Option<&SystemResourceState>,
) -> DeviceMapData {
    let text = |value: &mikrotik_model::ascii::AsciiString| Box::<str>::from(value.to_string());
    let health = health
        .iter()
        .filter_map(|row| {
            Some(HealthRow {
                name: text(row.name.as_ref()?),
                value: text(row.value.as_ref()?),
                unit: row._type.as_ref().map(text),
            })
        })
        .collect();
    let resources = resources.map(|resource| Resources {
        uptime: resource.uptime,
        cpu_load: resource.cpu_load.map(u32::from).unwrap_or_default(),
        free_memory: resource.free_memory.unwrap_or_default(),
        total_memory: resource.total_memory.unwrap_or_default(),
        free_hdd_space: resource.free_hdd_space.unwrap_or_default(),
        total_hdd_space: resource.total_hdd_space.unwrap_or_default(),
        version: text(&resource.version),
        board_name: resource.board_name.as_ref().map(text).unwrap_or_default(),
    });
    DeviceMapData {
        health,
        resources,
        poe: PoePorts::default(),
        poe_load: Box::default(),
    }
}

#[cfg(test)]
mod test;
