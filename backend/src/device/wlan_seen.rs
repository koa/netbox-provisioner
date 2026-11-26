//! Scope: The WLAN clients each access point had when its controller was last read, kept in
//! the process (E36); not reading them (`device::metrics`), nor showing them
//! (`graphql::resident::live`).
//!
//! Every read of a device's WLAN clients (a scrape of `/metrics`, „Geräte im Netz“, the page
//! „Zustand“) notes them by the access point they are on (`WlanClient::ap`, the device itself
//! for its own radios). An access point gone from the controller keeps the clients of the last
//! read it was in, with that read's time: an access point that stops answering takes its
//! clients with it, and the residents see which. An access point without clients is absent
//! too, so the time tells how old the list is. Entries not seen for a day go.
use super::metrics::WlanClient;
use crate::{lock::LockAnyway, topology::DeviceId};
use mikrotik_model::MacAddress;
use std::{
    collections::BTreeMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::time::Instant;

/// An entry not seen for this long goes.
const KEPT: Duration = Duration::from_secs(24 * 3600);

/// A client as its access point last had it.
#[derive(Debug, Clone, PartialEq)]
pub struct SeenClient {
    pub mac_address: MacAddress,
    pub ssid: Box<str>,
}

/// The clients an access point had at the last read of its controller that listed it.
#[derive(Debug, Clone)]
pub struct SeenAt {
    pub clients: Box<[SeenClient]>,
    pub at: Instant,
}

/// An access point at a controller, by its identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    controller: DeviceId,
    access_point: Box<str>,
}

/// The clients of every access point, kept in the process.
#[derive(Default)]
pub struct Seen {
    by_ap: BTreeMap<Key, SeenAt>,
}

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Mutex::default);

impl Seen {
    /// Notes `clients`, as `controller` (named `name`) gave them `at`.
    pub fn note(&mut self, controller: DeviceId, name: &str, clients: &[WlanClient], at: Instant) {
        let mut by_ap = BTreeMap::<Box<str>, Vec<SeenClient>>::new();
        for client in clients {
            let access_point = match client.ap.is_empty() {
                true => name.into(),
                false => client.ap.clone(),
            };
            by_ap.entry(access_point).or_default().push(SeenClient {
                mac_address: client.mac_address,
                ssid: client.ssid.clone(),
            });
        }
        for (access_point, clients) in by_ap {
            let key = Key {
                controller,
                access_point,
            };
            let clients = clients.into();
            self.by_ap.insert(key, SeenAt { clients, at });
        }
        self.by_ap
            .retain(|_, seen| at.saturating_duration_since(seen.at) < KEPT);
    }
    /// What `access_point` had at the last read of `controller` that listed it.
    pub fn of(&self, controller: DeviceId, access_point: &str) -> Option<&SeenAt> {
        self.by_ap.get(&Key {
            controller,
            access_point: access_point.into(),
        })
    }
}

/// Notes the clients `controller` gave now.
pub fn note(controller: DeviceId, name: &str, clients: &[WlanClient]) {
    SEEN.lock_anyway()
        .note(controller, name, clients, Instant::now());
}

/// What `access_point` had at the last read of `controller` that listed it.
pub fn of(controller: DeviceId, access_point: &str) -> Option<SeenAt> {
    SEEN.lock_anyway().of(controller, access_point).cloned()
}

#[cfg(test)]
mod test;
