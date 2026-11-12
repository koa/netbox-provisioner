//! Scope: Reading the link-state database from one router; not parsing it (`lsa`).
//!
//! Reading the link-state database from one router of a site: the management proxy's device
//! if it is there (it is reached directly), else the site's OSPF routers by name, the first
//! that answers. As a fact (`FactKind::Lsdb`), so the map's viewers share a reading.

use super::{edges::Lsdb, lsa::Lsa};
use crate::{
    Error,
    config::CONFIG,
    device::raw,
    facts::{self, Fact, FactKey, FactKind, Freshness},
    topology::access::{DeviceTarget, device::DeviceAccess},
};
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
    pub lsdb: Fact<Lsdb>,
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
        let ask = async move {
            let api = accessible.connect_again().await?;
            Ok(raw::rows(&api, "/routing/ospf/lsa/print")
                .await?
                .into_iter()
                .filter_map(|row| Lsa::from_row(|key| raw::field(&row, key).map(String::from)))
                .collect::<Lsdb>())
        };
        let key = FactKey::new(FactKind::Lsdb, Some(device.id()), "");
        let fact = facts::get(key, Freshness::MaxAge(LSDB_AGE), ask).await;
        match fact.value {
            Ok(_) => {
                return Ok(Some(LsdbRead {
                    device: device.clone(),
                    lsdb: fact,
                }));
            }
            Err(_) => failure = fact.result().err(),
        }
    }
    failure.map_or(Ok(None), Err)
}
