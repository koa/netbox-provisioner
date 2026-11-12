//! Scope: What a reboot or an update must wait for (PoE chains, the ways to the devices) and the
//! leases holding them; not the update itself (`update`).
//!
//! What a reboot must wait for (`docs/concepts/network-update.md`). A device that powers
//! another one (NetBox `poe_mode=pse` on the port cabled to it) must not reboot while that one
//! updates: the consumer has the packages on its flash from the upload on and installs them at
//! the next boot; losing power then can brick it. And a device must not reboot while one the
//! provisioner reaches only through it updates: the update would lose its device in the middle
//! of an upload or of waiting for its reboot.
//!
//! Each update holds an [`UpdateLease`] from before the upload to the end, each reboot of an
//! update a [`RebootLease`] until the device is back. A reboot waits until none of the devices
//! the device powers (directly or further down the chain) holds an update lease and every
//! device holding one is still reached ([`Paths`]) without it and the devices rebooting
//! already; an update waits to start while one of the devices powering it reboots or the
//! rebooting devices cut it off. Both are checked and taken under one lock, so the two can't
//! cross. The leases live in the provisioner process: updates started elsewhere (or before a
//! restart) aren't seen.
use crate::{
    config::CONFIG,
    job::Log,
    lock::LockAnyway,
    network_state::{
        edges::{EdgeState, edges},
        read::{read, readers},
    },
    topology::{access::device::DeviceAccess, management::ManagementTree},
};
use async_graphql::{Enum, SimpleObject};
use log::debug;
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, time::timeout};

/// How long an update waits for the link-state database before it takes every way NetBox has.
const LSDB_WAIT: Duration = Duration::from_secs(10);

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
pub struct WaitingForDevice {
    pub device: Box<str>,
    pub reason: DeviceWait,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Enum)]
pub enum DeviceWait {
    /// a device powering this one reboots; the update starts when it is back
    SupplierRebooting,
    /// a device this one powers updates; this one reboots when that is done
    ConsumerUpdating,
    /// a rebooting device cuts this one off; the update starts when it is back
    BehindRebooting,
    /// a device reached only through this one (and those rebooting) updates; this one reboots
    /// when that is done
    BehindUpdating,
}

/// The devices running an update and the devices rebooting for one, by name.
#[derive(Default)]
struct Leases {
    updating: Vec<Box<str>>,
    rebooting: Vec<Box<str>>,
}

static LEASES: LazyLock<(Mutex<Leases>, Notify)> = LazyLock::new(Default::default);

/// The ways the provisioner reaches the devices, by name: from the management roots
/// (`ManagementTree`: the management proxy's device and those announcing the default route)
/// over NetBox's cables and wireless links, without those OSPF says are down now. A way over
/// a device without OSPF (a switch carrying a link) counts, the LSDB says nothing about it.
#[derive(Debug, Default, Clone)]
pub struct Paths {
    roots: BTreeSet<Box<str>>,
    links: HashMap<Box<str>, BTreeSet<Box<str>>>,
}

impl Paths {
    /// The ways in the network of `device`, with the LSDB of one of its routers; every way
    /// NetBox has if none answers within `LSDB_WAIT`.
    pub async fn of(device: &DeviceAccess) -> Paths {
        let devices = device.topology().list_devices().collect::<Vec<_>>();
        let lsdb = timeout(LSDB_WAIT, read(&readers(&devices)))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .and_then(|read| read.lsdb.value.ok());
        let proxy = CONFIG.management_proxy.as_ref().map(|proxy| &*proxy.device);
        let tree = ManagementTree::of(device.topology(), proxy);
        let topology = device.topology();
        let mut paths = Paths {
            roots: devices
                .iter()
                .filter(|device| tree.is_root(device.id()))
                .map(|device| device.name().into())
                .collect(),
            links: HashMap::new(),
        };
        for edge in edges(&devices, lsdb.as_deref().unwrap_or(&Default::default())) {
            let up = match edge.state {
                EdgeState::Up | EdgeState::V3Down | EdgeState::NotOspf => true,
                EdgeState::Down(_) => lsdb.is_none(),
            };
            let end = |interface| {
                topology
                    .get_interface_by_id(interface)
                    .and_then(|interface| interface.device())
            };
            if let (true, Some(a), Some(b)) = (up, end(edge.a), end(edge.b)) {
                paths.link(a.name(), b.name());
            }
        }
        paths
    }
    fn link(&mut self, a: &str, b: &str) {
        if a != b {
            self.links.entry(a.into()).or_default().insert(b.into());
            self.links.entry(b.into()).or_default().insert(a.into());
        }
    }
    #[cfg(test)]
    fn new(roots: &[&str], links: &[(&str, &str)]) -> Paths {
        let mut paths = Paths {
            roots: roots.iter().map(|&root| root.into()).collect(),
            links: HashMap::new(),
        };
        for (a, b) in links {
            paths.link(a, b);
        }
        paths
    }
    /// Whether a root reaches `device` without passing `away`.
    fn reaches(&self, device: &str, away: &BTreeSet<&str>) -> bool {
        let mut seen = self
            .roots
            .iter()
            .map(|root| &**root)
            .filter(|root| !away.contains(root))
            .collect::<HashSet<_>>();
        let mut todo = seen.iter().copied().collect::<VecDeque<_>>();
        while let Some(near) = todo.pop_front() {
            if near == device {
                return true;
            }
            for far in self.links.get(near).into_iter().flatten() {
                if !away.contains(&**far) && seen.insert(far) {
                    todo.push_back(far);
                }
            }
        }
        false
    }
    /// The hops from the nearest root to each device a root reaches.
    pub fn depths(&self) -> HashMap<Box<str>, u32> {
        let mut depths = self
            .roots
            .iter()
            .map(|root| (root.clone(), 0))
            .collect::<HashMap<_, _>>();
        let mut todo = self.roots.iter().cloned().collect::<VecDeque<_>>();
        while let Some(near) = todo.pop_front() {
            let next = depths[&near] + 1;
            for far in self.links.get(&near).into_iter().flatten() {
                if !depths.contains_key(far) {
                    depths.insert(far.clone(), next);
                    todo.push_back(far.clone());
                }
            }
        }
        depths
    }
    /// The devices a root reaches only through `device`.
    pub fn behind(&self, device: &str) -> BTreeSet<Box<str>> {
        self.cut_off(
            self.links.keys().map(|device| &**device),
            &BTreeSet::from([device]),
        )
    }
    /// Of `devices`, those a root reaches now but not with `away` gone. A device reached
    /// without a way NetBox knows (not at all now) is no one's to wait for.
    fn cut_off<'a>(
        &self,
        devices: impl IntoIterator<Item = &'a str>,
        away: &BTreeSet<&str>,
    ) -> BTreeSet<Box<str>> {
        devices
            .into_iter()
            .filter(|device| !away.contains(device))
            .filter(|device| self.reaches(device, &BTreeSet::new()) && !self.reaches(device, away))
            .map(Box::from)
            .collect()
    }
}

/// Where a device sits in the PoE chains and the ways of the network.
#[derive(Debug, Clone, Default)]
pub struct PowerChain {
    device: Box<str>,
    /// the devices that power this one, directly or further up
    suppliers: BTreeSet<Box<str>>,
    /// the devices this one powers, directly or further down
    consumers: BTreeSet<Box<str>>,
    paths: Arc<Paths>,
}

impl PowerChain {
    /// A device without PoE relations and ways (test setups).
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
            paths: Default::default(),
        }
    }
    #[cfg(test)]
    fn on_paths(device: &str, paths: &Arc<Paths>) -> PowerChain {
        PowerChain {
            device: device.into(),
            paths: paths.clone(),
            ..Default::default()
        }
    }
    pub async fn of(device: &DeviceAccess) -> PowerChain {
        let paths = Paths::of(device).await;
        debug!("{}: {paths:?}", device.name());
        PowerChain {
            device: device.name().into(),
            suppliers: names(supplier_devices(device)),
            consumers: names(consumer_devices(device)),
            paths: Arc::new(paths),
        }
    }
    /// Waits until no supplier reboots and the rebooting devices don't cut it off, then marks
    /// the device as updating.
    pub async fn updating(&self, log: &Log) -> UpdateLease {
        if !self.suppliers.is_empty() || !self.consumers.is_empty() {
            log.emit(PoeChain {
                suppliers: self.suppliers.iter().cloned().collect(),
                consumers: self.consumers.iter().cloned().collect(),
            });
        }
        let device = self.device.clone();
        acquire(log, |leases| {
            let mut waits = waits(
                blocking(&leases.rebooting, &self.suppliers),
                DeviceWait::SupplierRebooting,
            );
            if waits.is_empty() {
                waits = waits_behind(&self.paths, &device, &leases.rebooting);
            }
            if waits.is_empty() {
                leases.updating.push(device.clone());
            }
            waits
        })
        .await;
        UpdateLease(self.device.clone())
    }
    /// Waits until no consumer updates and every device updating is still reached without
    /// this one, then marks the device as rebooting.
    pub async fn rebooting(&self, log: &Log) -> RebootLease {
        reboot_lease(&self.device, &self.consumers, &self.paths, log).await
    }
    /// Waits until neither the device nor a consumer updates, then marks `supplier`, which
    /// feeds it, as rebooting: its power is cut (`power_cycle`), which bricks a device
    /// installing packages as well, and an update of it (or of one it powers) waits for the
    /// lease like for a supplier's reboot.
    pub async fn power_cut(&self, supplier: &str, log: &Log) -> RebootLease {
        let mut cut = self.consumers.clone();
        cut.insert(self.device.clone());
        reboot_lease(supplier, &cut, &self.paths, log).await
    }
}

/// Waits until none of `waiting_for` updates and every device updating is reached without
/// `device` and the devices rebooting, then marks `device` as rebooting.
async fn reboot_lease(
    device: &str,
    waiting_for: &BTreeSet<Box<str>>,
    paths: &Paths,
    log: &Log,
) -> RebootLease {
    let device: Box<str> = device.into();
    acquire(log, |leases| {
        let mut waits = waits(
            blocking(&leases.updating, waiting_for),
            DeviceWait::ConsumerUpdating,
        );
        if waits.is_empty() {
            let mut away = leases
                .rebooting
                .iter()
                .map(|device| &**device)
                .collect::<BTreeSet<_>>();
            away.insert(&device);
            waits = self::waits(
                paths.cut_off(leases.updating.iter().map(|device| &**device), &away),
                DeviceWait::BehindUpdating,
            );
        }
        if waits.is_empty() {
            leases.rebooting.push(device.clone());
        }
        waits
    })
    .await;
    RebootLease(device)
}

/// What an update of `device` waits for among the devices `rebooting`: those cutting it off
/// alone, or all of them where only together they do.
fn waits_behind(paths: &Paths, device: &str, rebooting: &[Box<str>]) -> Vec<WaitingForDevice> {
    let all = rebooting
        .iter()
        .map(|device| &**device)
        .collect::<BTreeSet<_>>();
    if paths.cut_off([device], &all).is_empty() {
        return Vec::new();
    }
    let alone = rebooting
        .iter()
        .filter(|rebooting| {
            !paths
                .cut_off([device], &BTreeSet::from([&***rebooting]))
                .is_empty()
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let cutting = if alone.is_empty() {
        rebooting.iter().cloned().collect()
    } else {
        alone
    };
    waits(cutting, DeviceWait::BehindRebooting)
}

fn waits(devices: BTreeSet<Box<str>>, reason: DeviceWait) -> Vec<WaitingForDevice> {
    devices
        .into_iter()
        .map(|device| WaitingForDevice { device, reason })
        .collect()
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

/// The devices `device` powers, directly or further down.
pub fn consumer_devices(device: &DeviceAccess) -> Vec<DeviceAccess> {
    walk(device, |device| {
        device
            .interfaces()
            .iter()
            .filter(|port| port.enable_poe())
            .flat_map(|port| port.connected_interfaces())
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
async fn acquire(log: &Log, mut try_take: impl FnMut(&mut Leases) -> Vec<WaitingForDevice>) {
    let (leases, changed) = &*LEASES;
    let mut logged = HashSet::new();
    loop {
        // registered before the check, so a release in between still wakes us
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let blocking = try_take(&mut leases.lock_anyway());
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
        let mut leases = leases.lock_anyway();
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
    use super::{DeviceWait, Paths, PowerChain, WaitingForDevice};
    use crate::job::{Event, Log};
    use std::{sync::Arc, time::Duration};
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
            [Event::WaitingForDevice(WaitingForDevice {
                device: "t1-behind-ap".into(),
                reason: DeviceWait::ConsumerUpdating,
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
        assert!(
            timeout(Duration::from_millis(100), cut.as_mut())
                .await
                .is_err()
        );
        drop(update);
        let cut = timeout(Duration::from_secs(1), cut)
            .await
            .expect("cut after the update");
        assert_eq!(
            cut_log.events(),
            [Event::WaitingForDevice(WaitingForDevice {
                device: "t4-ap".into(),
                reason: DeviceWait::ConsumerUpdating,
            })]
        );
        let update = ap.updating(&log);
        tokio::pin!(update);
        assert!(
            timeout(Duration::from_millis(100), update.as_mut())
                .await
                .is_err()
        );
        drop(cut);
        let _update = timeout(Duration::from_secs(1), update)
            .await
            .expect("update after the cut");
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
            !events
                .iter()
                .any(|e| matches!(e, Event::WaitingForDevice(_))),
            "{events:?}"
        );
    }

    /// entry -- a -- target (the CHR chain of `ospf_chain.rs`): `a` reboots only after the
    /// update of `target`, and `target`'s update waits while `a` reboots.
    #[tokio::test]
    async fn a_device_reboots_after_the_update_of_one_behind_it() {
        let paths = Arc::new(Paths::new(
            &["t5-entry"],
            &[("t5-entry", "t5-a"), ("t5-a", "t5-target")],
        ));
        let a = PowerChain::on_paths("t5-a", &paths);
        let target = PowerChain::on_paths("t5-target", &paths);
        let log = Log::new("t5");
        let update = target.updating(&log).await;
        let a_log = Log::new("t5");
        let reboot = a.rebooting(&a_log);
        tokio::pin!(reboot);
        assert!(
            timeout(Duration::from_millis(100), reboot.as_mut())
                .await
                .is_err()
        );
        drop(update);
        let reboot = timeout(Duration::from_secs(1), reboot)
            .await
            .expect("reboot after the update");
        assert_eq!(
            a_log.events(),
            [Event::WaitingForDevice(WaitingForDevice {
                device: "t5-target".into(),
                reason: DeviceWait::BehindUpdating,
            })]
        );
        let target_log = Log::new("t5");
        let update = target.updating(&target_log);
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
        assert!(
            target_log
                .events()
                .contains(&Event::WaitingForDevice(WaitingForDevice {
                    device: "t5-a".into(),
                    reason: DeviceWait::BehindRebooting,
                }))
        );
    }

    /// Reached over two ways (lab-sxt-a over lab-np1 or the radio bridge of lab-np2): either
    /// side reboots while the device updates, not both at once.
    #[tokio::test]
    async fn of_two_ways_one_reboots_at_a_time() {
        let paths = Arc::new(Paths::new(
            &["t6-root"],
            &[
                ("t6-root", "t6-left"),
                ("t6-root", "t6-right"),
                ("t6-left", "t6-target"),
                ("t6-right", "t6-target"),
            ],
        ));
        let log = Log::new("t6");
        let _update = PowerChain::on_paths("t6-target", &paths)
            .updating(&log)
            .await;
        let left = timeout(
            Duration::from_secs(1),
            PowerChain::on_paths("t6-left", &paths).rebooting(&log),
        )
        .await
        .expect("one way stays");
        let right_log = Log::new("t6");
        let right = PowerChain::on_paths("t6-right", &paths);
        let reboot = right.rebooting(&right_log);
        tokio::pin!(reboot);
        assert!(
            timeout(Duration::from_millis(100), reboot.as_mut())
                .await
                .is_err()
        );
        drop(left);
        let _reboot = timeout(Duration::from_secs(1), reboot)
            .await
            .expect("the other way is back");
    }

    /// A device updating that no root reaches over NetBox's ways (reached some other way)
    /// holds no reboot back.
    #[tokio::test]
    async fn a_device_off_the_ways_holds_nothing_back() {
        let paths = Arc::new(Paths::new(&["t7-root"], &[("t7-root", "t7-a")]));
        let log = Log::new("t7");
        let _update = PowerChain::on_paths("t7-elsewhere", &paths)
            .updating(&log)
            .await;
        let _reboot = timeout(
            Duration::from_secs(1),
            PowerChain::on_paths("t7-root", &paths).rebooting(&log),
        )
        .await
        .expect("no wait");
    }
}
