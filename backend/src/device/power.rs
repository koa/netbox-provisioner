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
use crate::{job::Log, topology::access::device::DeviceAccess};
use async_graphql::{Enum, SimpleObject};
use std::{
    collections::{BTreeSet, HashSet},
    sync::{LazyLock, Mutex, PoisonError},
};
use tokio::sync::Notify;

/// The devices up and down the PoE chains of the device that updates.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct PoeChain {
    /// the devices powering it, directly or further up
    pub suppliers: Box<[Box<str>]>,
    /// the devices it powers, directly or further down
    pub consumers: Box<[Box<str>]>,
}

/// The update waits for `device`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, SimpleObject)]
pub struct WaitingForPower {
    pub device: Box<str>,
    pub reason: PowerWait,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum PowerWait {
    /// a device powering this one reboots; the update starts when it is back
    SupplierRebooting,
    /// a device this one powers updates; this one reboots when that is done
    ConsumerUpdating,
}

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
            suppliers: names(supplier_devices(device)),
            consumers: names(walk(device, |device| {
                device
                    .interfaces()
                    .iter()
                    .filter(|port| port.enable_poe())
                    .flat_map(|port| port.connected_interfaces())
                    .filter_map(|far| far.device())
                    .collect()
            })),
        }
    }
    /// Waits until no supplier reboots, then marks the device as updating.
    pub async fn updating(&self, log: &Log) -> UpdateLease {
        if !self.suppliers.is_empty() || !self.consumers.is_empty() {
            log.emit(PoeChain {
                suppliers: self.suppliers.iter().cloned().collect(),
                consumers: self.consumers.iter().cloned().collect(),
            });
        }
        let device = self.device.clone();
        acquire(log, |leases| {
            let blocking = blocking(&leases.rebooting, &self.suppliers);
            if blocking.is_empty() {
                leases.updating.push(device.clone());
            }
            blocking
                .into_iter()
                .map(|device| WaitingForPower {
                    device,
                    reason: PowerWait::SupplierRebooting,
                })
                .collect()
        })
        .await;
        UpdateLease(self.device.clone())
    }
    /// Waits until no consumer updates, then marks the device as rebooting.
    pub async fn rebooting(&self, log: &Log) -> RebootLease {
        reboot_lease(&self.device, &self.consumers, log).await
    }
    /// Waits until neither the device nor a consumer updates, then marks `supplier`, which
    /// feeds it, as rebooting: its power is cut (`power_cycle`), which bricks a device
    /// installing packages as well, and an update of it (or of one it powers) waits for the
    /// lease like for a supplier's reboot.
    pub async fn power_cut(&self, supplier: &str, log: &Log) -> RebootLease {
        let mut cut = self.consumers.clone();
        cut.insert(self.device.clone());
        reboot_lease(supplier, &cut, log).await
    }
}

/// Waits until none of `waiting_for` updates, then marks `device` as rebooting.
async fn reboot_lease(device: &str, waiting_for: &BTreeSet<Box<str>>, log: &Log) -> RebootLease {
    let device: Box<str> = device.into();
    acquire(log, |leases| {
        let blocking = blocking(&leases.updating, waiting_for);
        if blocking.is_empty() {
            leases.rebooting.push(device.clone());
        }
        blocking
            .into_iter()
            .map(|device| WaitingForPower {
                device,
                reason: PowerWait::ConsumerUpdating,
            })
            .collect()
    })
    .await;
    RebootLease(device)
}

/// The devices powering `device`, directly or further up.
pub fn supplier_devices(device: &DeviceAccess) -> Vec<DeviceAccess> {
    walk(device, |device| {
        device
            .interfaces()
            .iter()
            .flat_map(|port| port.connected_interfaces())
            .filter(|far| far.enable_poe())
            .filter_map(|far| far.device())
            .collect()
    })
}

fn names(devices: Vec<DeviceAccess>) -> BTreeSet<Box<str>> {
    devices.iter().map(|device| device.name().into()).collect()
}

/// The devices reachable from `device` over `next`, without `device` itself, nearest first.
fn walk(
    device: &DeviceAccess,
    next: impl Fn(&DeviceAccess) -> Vec<DeviceAccess>,
) -> Vec<DeviceAccess> {
    let mut seen = HashSet::from([device.id()]);
    let mut found = Vec::new();
    let mut todo = std::collections::VecDeque::from([device.clone()]);
    while let Some(device) = todo.pop_front() {
        for other in next(&device) {
            if seen.insert(other.id()) {
                found.push(other.clone());
                todo.push_back(other);
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
async fn acquire(log: &Log, mut try_take: impl FnMut(&mut Leases) -> Vec<WaitingForPower>) {
    let (leases, changed) = &*LEASES;
    let mut logged = HashSet::new();
    loop {
        // registered before the check, so a release in between still wakes us
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let blocking = try_take(&mut leases.lock().unwrap_or_else(PoisonError::into_inner));
        if blocking.is_empty() {
            return;
        }
        for reason in blocking {
            if logged.insert(reason.clone()) {
                log.emit(reason);
            }
        }
        notified.await;
    }
}

fn release(list: impl FnOnce(&mut Leases) -> &mut Vec<Box<str>>, device: &str) {
    let (leases, changed) = &*LEASES;
    {
        let mut leases = leases.lock().unwrap_or_else(PoisonError::into_inner);
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
    use super::{PowerChain, PowerWait, WaitingForPower};
    use crate::job::{Event, Log};
    use std::time::Duration;
    use tokio::time::timeout;

    // device names are unique per test, the leases are shared by all tests of the process

    #[tokio::test]
    async fn a_supplier_reboots_after_its_consumers_updates() {
        let supplier = PowerChain::new("t1-switch", &[], &["t1-ap", "t1-behind-ap"]);
        let consumer = PowerChain::new("t1-behind-ap", &["t1-ap", "t1-switch"], &[]);
        let log = Log::new("t1");
        let update = consumer.updating(&log).await;
        let supplier_log = Log::new("t1");
        {
            let reboot = supplier.rebooting(&supplier_log);
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
            supplier_log.events(),
            [Event::WaitingForPower(WaitingForPower {
                device: "t1-behind-ap".into(),
                reason: PowerWait::ConsumerUpdating,
            })]
        );
    }

    /// Cutting a device's power waits for its own update, and its next update waits for the
    /// power to be back.
    #[tokio::test]
    async fn a_power_cut_waits_for_the_update_of_the_device() {
        let ap = PowerChain::new("t4-ap", &["t4-switch"], &[]);
        let log = Log::new("t4");
        let update = ap.updating(&log).await;
        let cut_log = Log::new("t4");
        let cut = ap.power_cut("t4-switch", &cut_log);
        tokio::pin!(cut);
        assert!(timeout(Duration::from_millis(100), cut.as_mut()).await.is_err());
        drop(update);
        let cut = timeout(Duration::from_secs(1), cut).await.expect("cut after the update");
        assert_eq!(
            cut_log.events(),
            [Event::WaitingForPower(WaitingForPower {
                device: "t4-ap".into(),
                reason: PowerWait::ConsumerUpdating,
            })]
        );
        let update = ap.updating(&log);
        tokio::pin!(update);
        assert!(timeout(Duration::from_millis(100), update.as_mut()).await.is_err());
        drop(cut);
        let _update = timeout(Duration::from_secs(1), update).await.expect("update after the cut");
    }

    #[tokio::test]
    async fn an_update_waits_for_its_suppliers_reboot() {
        let supplier = PowerChain::new("t2-switch", &[], &["t2-ap"]);
        let consumer = PowerChain::new("t2-ap", &["t2-switch"], &[]);
        let log = Log::new("t2");
        let reboot = supplier.rebooting(&log).await;
        let consumer_log = Log::new("t2");
        let update = consumer.updating(&consumer_log);
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
        let log = Log::new("t3");
        let _other_update = other.updating(&log).await;
        // a device reboots within its own update
        let _update = switch.updating(&log).await;
        let _reboot = timeout(Duration::from_secs(1), switch.rebooting(&log))
            .await
            .expect("no wait");
        let events = log.events();
        assert!(
            !events.iter().any(|e| matches!(e, Event::WaitingForPower(_))),
            "{events:?}"
        );
    }
}
