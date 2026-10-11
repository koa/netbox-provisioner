//! PoE during updates: a device that powers another one (NetBox `poe_mode=pse` on the port
//! cabled to it) must not reboot while that one updates. The consumer has the packages on its
//! flash from the upload on and installs them at the next boot; losing power then can brick it.
//!
//! Each update holds an [`UpdateLease`] from before the upload to the end, each reboot of an
//! update a [`RebootLease`] until the device is back. A reboot waits until none of the devices
//! the device powers (directly or further down the chain) holds an update lease, and an update
//! waits to start while one of the devices powering it reboots. Both are checked and taken
//! under one lock, so the two can't cross. The leases live in the provisioner process: updates
//! started elsewhere (or before a restart) aren't seen.
use crate::topology::access::device::DeviceAccess;
use log::info;
use std::{
    collections::{BTreeSet, HashSet},
    fmt::Write,
    sync::{LazyLock, Mutex},
};
use tokio::sync::Notify;

/// The devices running an update and the devices rebooting for one, by name.
#[derive(Default)]
struct Leases {
    updating: Vec<Box<str>>,
    rebooting: Vec<Box<str>>,
}

static LEASES: LazyLock<(Mutex<Leases>, Notify)> = LazyLock::new(Default::default);

/// Where a device sits in the PoE chains of the network.
#[derive(Debug, Clone, Default)]
pub struct PowerChain {
    device: Box<str>,
    /// the devices that power this one, directly or further up
    suppliers: BTreeSet<Box<str>>,
    /// the devices this one powers, directly or further down
    consumers: BTreeSet<Box<str>>,
}

impl PowerChain {
    /// A device without PoE relations (test setups).
    pub fn unpowered(device: &str) -> PowerChain {
        PowerChain {
            device: device.into(),
            ..Default::default()
        }
    }
    #[cfg(test)]
    fn new(device: &str, suppliers: &[&str], consumers: &[&str]) -> PowerChain {
        PowerChain {
            device: device.into(),
            suppliers: suppliers.iter().map(|&d| d.into()).collect(),
            consumers: consumers.iter().map(|&d| d.into()).collect(),
        }
    }
    pub fn of(device: &DeviceAccess) -> PowerChain {
        PowerChain {
            device: device.name().into(),
            suppliers: walk(device, |device| {
                device
                    .interfaces()
                    .iter()
                    .flat_map(|port| port.connected_interfaces())
                    .filter(|far| far.enable_poe())
                    .filter_map(|far| far.device())
                    .collect()
            }),
            consumers: walk(device, |device| {
                device
                    .interfaces()
                    .iter()
                    .filter(|port| port.enable_poe())
                    .flat_map(|port| port.connected_interfaces())
                    .filter_map(|far| far.device())
                    .collect()
            }),
        }
    }
    /// Waits until no supplier reboots, then marks the device as updating.
    pub async fn updating(&self, log: &mut String) -> UpdateLease {
        let names = |devices: &BTreeSet<Box<str>>| {
            devices.iter().map(|d| &**d).collect::<Vec<_>>().join(", ")
        };
        if !self.suppliers.is_empty() {
            let _ = writeln!(log, "powered by {}", names(&self.suppliers));
        }
        if !self.consumers.is_empty() {
            let _ = writeln!(log, "powers {}", names(&self.consumers));
        }
        let device = self.device.clone();
        acquire(log, |leases| {
            let blocking = blocking(&leases.rebooting, &self.suppliers);
            if blocking.is_empty() {
                leases.updating.push(device.clone());
            }
            blocking
                .iter()
                .map(|supplier| {
                    format!(
                        "{supplier} (powers {}) to come back from its reboot",
                        self.device
                    )
                })
                .collect()
        })
        .await;
        UpdateLease(self.device.clone())
    }
    /// Waits until no consumer updates, then marks the device as rebooting.
    pub async fn rebooting(&self, log: &mut String) -> RebootLease {
        let device = self.device.clone();
        acquire(log, |leases| {
            let blocking = blocking(&leases.updating, &self.consumers);
            if blocking.is_empty() {
                leases.rebooting.push(device.clone());
            }
            blocking
                .iter()
                .map(|consumer| {
                    format!(
                        "{consumer} (powered by {}) to finish its update",
                        self.device
                    )
                })
                .collect()
        })
        .await;
        RebootLease(self.device.clone())
    }
}

/// The devices reachable from `device` over `next`, without `device` itself.
fn walk(
    device: &DeviceAccess,
    next: impl Fn(&DeviceAccess) -> Vec<DeviceAccess>,
) -> BTreeSet<Box<str>> {
    let mut seen = HashSet::from([device.id()]);
    let mut found = BTreeSet::new();
    let mut todo = vec![device.clone()];
    while let Some(device) = todo.pop() {
        for other in next(&device) {
            if seen.insert(other.id()) {
                found.insert(other.name().into());
                todo.push(other);
            }
        }
    }
    found
}

/// The leased devices in `related`, deduplicated.
fn blocking(leased: &[Box<str>], related: &BTreeSet<Box<str>>) -> BTreeSet<Box<str>> {
    leased
        .iter()
        .filter(|device| related.contains(*device))
        .cloned()
        .collect()
}

/// Runs `try_take` under the lock until it takes the lease (returns nothing to wait for),
/// logging each thing it waits for once.
async fn acquire(log: &mut String, mut try_take: impl FnMut(&mut Leases) -> Vec<String>) {
    let (leases, changed) = &*LEASES;
    let mut logged = HashSet::new();
    loop {
        // registered before the check, so a release in between still wakes us
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let blocking = try_take(&mut leases.lock().expect("power leases"));
        if blocking.is_empty() {
            return;
        }
        for reason in blocking {
            if !logged.contains(&reason) {
                info!("waiting for {reason}");
                let _ = writeln!(log, "waiting for {reason}");
                logged.insert(reason);
            }
        }
        notified.await;
    }
}

fn release(list: impl FnOnce(&mut Leases) -> &mut Vec<Box<str>>, device: &str) {
    let (leases, changed) = &*LEASES;
    {
        let mut leases = leases.lock().expect("power leases");
        let list = list(&mut leases);
        if let Some(at) = list.iter().position(|d| &**d == device) {
            list.remove(at);
        }
    }
    changed.notify_waiters();
}

/// The device updates; released when dropped.
pub struct UpdateLease(Box<str>);

impl Drop for UpdateLease {
    fn drop(&mut self) {
        release(|leases| &mut leases.updating, &self.0);
    }
}

/// The device reboots for an update; released when dropped.
pub struct RebootLease(Box<str>);

impl Drop for RebootLease {
    fn drop(&mut self) {
        release(|leases| &mut leases.rebooting, &self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::PowerChain;
    use std::time::Duration;
    use tokio::time::timeout;

    // device names are unique per test, the leases are shared by all tests of the process

    #[tokio::test]
    async fn a_supplier_reboots_after_its_consumers_updates() {
        let supplier = PowerChain::new("t1-switch", &[], &["t1-ap", "t1-behind-ap"]);
        let consumer = PowerChain::new("t1-behind-ap", &["t1-ap", "t1-switch"], &[]);
        let mut log = String::new();
        let update = consumer.updating(&mut log).await;
        let mut supplier_log = String::new();
        {
            let reboot = supplier.rebooting(&mut supplier_log);
            tokio::pin!(reboot);
            assert!(
                timeout(Duration::from_millis(100), reboot.as_mut())
                    .await
                    .is_err()
            );
            drop(update);
            let _reboot = timeout(Duration::from_secs(1), reboot)
                .await
                .expect("reboot after the update");
        }
        assert_eq!(
            supplier_log,
            "waiting for t1-behind-ap (powered by t1-switch) to finish its update\n"
        );
    }

    #[tokio::test]
    async fn an_update_waits_for_its_suppliers_reboot() {
        let supplier = PowerChain::new("t2-switch", &[], &["t2-ap"]);
        let consumer = PowerChain::new("t2-ap", &["t2-switch"], &[]);
        let mut log = String::new();
        let reboot = supplier.rebooting(&mut log).await;
        let mut consumer_log = String::new();
        let update = consumer.updating(&mut consumer_log);
        tokio::pin!(update);
        assert!(
            timeout(Duration::from_millis(100), update.as_mut())
                .await
                .is_err()
        );
        drop(reboot);
        let _update = timeout(Duration::from_secs(1), update)
            .await
            .expect("update after the reboot");
    }

    #[tokio::test]
    async fn unrelated_devices_and_the_device_itself_dont_wait() {
        let switch = PowerChain::new("t3-switch", &[], &["t3-ap"]);
        let other = PowerChain::new("t3-other", &[], &[]);
        let mut log = String::new();
        let _other_update = other.updating(&mut log).await;
        // a device reboots within its own update
        let _update = switch.updating(&mut log).await;
        let _reboot = timeout(Duration::from_secs(1), switch.rebooting(&mut log))
            .await
            .expect("no wait");
        assert!(!log.contains("waiting"), "{log}");
    }
}
