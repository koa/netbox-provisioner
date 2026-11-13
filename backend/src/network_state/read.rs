//! Scope: Reading the link-state database from one router; not parsing it (`lsa`).
//!
//! Reading the link-state database from one router of a site: the management proxy's device
//! if it is there (it is reached directly), else the site's OSPF routers by name, the first
//! that answers. What it answers is a fact (`access::Reader::at`), so the map's viewers share a
//! reading.

use super::{edges::Lsdb, lsa::Lsa};
use crate::{
    Error,
    config::CONFIG,
    facts::Freshness,
    topology::access::{DeviceTarget, device::DeviceAccess},
};
use mikrotik_model::{model::RoutingOspfLsaState, resource::collect_resource};
use std::time::Duration;

/// How old a reading may be: the map asks every few seconds, reading 500 LSAs takes 0.04 s.
pub const LSDB_AGE: Duration = Duration::from_secs(5);

/// The routers to read from, in turn.
pub fn readers(devices: &[DeviceAccess]) -> Vec<DeviceAccess> {
    let proxy = CONFIG.management_proxy.as_ref().map(|proxy| &*proxy.device);
    let mut readers = devices
        .iter()
        .filter(|device| device.has_routeros() && device.interfaces().iter().any(|i| i.use_ospf()))
        .cloned()
        .collect::<Vec<_>>();
    readers.sort_by_key(|device| (Some(device.name()) != proxy, device.name().to_string()));
    readers
}

/// An LSDB and the device it is from ([`read`]).
pub struct LsdbRead {
    pub device: DeviceAccess,
    pub lsdb: Lsdb,
    /// when the device was read for it
    pub as_of: tokio::time::Instant,
}

/// The LSDB of the first of `readers` that answers, and which one it was; the last failure
/// if none does (none without readers).
pub async fn read(readers: &[DeviceAccess]) -> Result<Option<LsdbRead>, Error> {
    let mut failure = None;
    for device in readers {
        let accessible = match device.connect(DeviceTarget::default()).await {
            Ok(Some(accessible)) => accessible,
            Ok(None) => continue,
            Err(error) => {
                failure = Some(error);
                continue;
            }
        };
        let reader = accessible.reader().at(Freshness::MaxAge(LSDB_AGE));
        match collect_resource::<RoutingOspfLsaState>(&reader).await {
            Ok(rows) => {
                return Ok(Some(LsdbRead {
                    device: device.clone(),
                    lsdb: rows.iter().filter_map(Lsa::from_state).collect(),
                    as_of: reader.as_of(),
                }));
            }
            Err(error) => failure = Some(error.into()),
        }
    }
    failure.map_or(Ok(None), Err)
}
