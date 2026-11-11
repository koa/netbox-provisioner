//! Scope: Suspending and restoring the watchdogs that may fire while a device updates; not the
//! watchdogs' config (`ros::watchdog`).
//!
//! The watchdogs that may fire while a device updates (`docs/watchdog.md`). A device
//! with `/system/watchdog watch-address` reboots when that address stops answering for
//! `ping-timeout`; a PoE port with `power-cycle-ping` cuts the power of a device that stops
//! answering. An update cuts the device off while it installs its packages, so:
//! - the ports powering it would cut its power mid-install, which can brick it;
//! - a device powering it and watching an address behind it reboots and cuts its power too
//!   (berghof-burri bricked burri-schopf that way);
//! - the devices watching it, or watching their parent through it (`ManagementTree::depending_on`),
//!   reboot for nothing.
//!
//! The power leases ([`power`](super::power)) only hold back reboots the provisioner sends
//! itself. [`read`] shows what each of these [`watchers`] watches before the update. Asked to,
//! the update [`suspend`]s them for its duration (`watch-address=none`,
//! `power-cycle-ping-enabled=no`) and [`restore`]s them afterwards, whatever the update's
//! outcome. In case the provisioner doesn't get that far, a scheduler entry on the device
//! restores them by itself [`SAFETY_NET`] later; it is added before they go and removed after
//! they are back.
//!
//! Several jobs may need a device's watchdogs suspended at once (a rollout's updates behind the
//! same switch, two updates side by side): the first suspends them, the others join ([`HELD`]),
//! adding their own PoE ports, and only the last restores them. Each job restoring what it saw
//! let a job that finished first set the watchdogs again while another still installed, and two
//! jobs adding the scheduler entry at once failed (Berg 925, 2026-11-07).
use crate::lock::LockAnyway;
use crate::{
    Error,
    device::connect_for_job,
    device::{
        AccessibleDevice,
        raw::{command, field, rows},
    },
    device_state::JobPhase,
    error_info::ErrorInfo,
    graphql::scalars::ScalarDuration,
    job::Log,
    topology::{
        WatchdogTarget,
        access::{DeviceTarget, device::DeviceAccess},
        management::ManagementTree,
    },
};
use async_graphql::SimpleObject;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

/// The name of the scheduler entry restoring the watchdogs.
pub(crate) const NAME: &str = "netbox-provisioner-watchdog";
/// Longer than any update takes: two steps (RouterOS 6 to 7), each with the update fallback's
/// wait, then the firmware.
pub const SAFETY_NET: Duration = Duration::from_secs(2 * 60 * 60);

/// A PoE port power cycling what it powers when that stops answering.
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject, serde::Serialize, serde::Deserialize)]
pub struct PoePing {
    pub port: Box<str>,
    pub address: Box<str>,
    /// as RouterOS reports it
    pub timeout: Box<str>,
}

/// What a device's watchdogs watch, of what an update may cut off.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, SimpleObject, serde::Serialize, serde::Deserialize,
)]
pub struct Watching {
    /// the address `/system/watchdog` watches, if any
    pub watch_address: Option<Box<str>>,
    /// its ports power cycling the updating device
    pub poe_pings: Vec<PoePing>,
}

impl Watching {
    pub fn is_empty(&self) -> bool {
        self.watch_address.is_none() && self.poe_pings.is_empty()
    }
}

/// The watchdogs of a device that may fire while the one the page is about updates.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Watchdog {
    pub device: Box<str>,
    /// it powers the device, directly or further up
    pub supplier: bool,
    pub watching: Watching,
    /// why it couldn't be read
    pub problem: Option<ErrorInfo>,
}

/// `device` watches nothing the update may cut off: nothing to suspend.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogNotSet {
    pub device: Box<str>,
}

/// The update suspended the watchdogs of `device`; the scheduler entry restores them after
/// `safety_net` should the provisioner not.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogSuspended {
    pub device: Box<str>,
    pub watching: Watching,
    pub safety_net: ScalarDuration,
}

/// The watchdogs of `device` watch again.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogRestored {
    pub device: Box<str>,
    pub watching: Watching,
}

/// The watchdogs of `device` stay suspended: another job still needs them so.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogStillHeld {
    pub device: Box<str>,
    /// how many other jobs hold them
    pub others: u32,
}

/// The watchdogs of `device` couldn't be restored; its scheduler entry does it after the
/// safety net.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogNotRestored {
    pub device: Box<str>,
    pub watching: Watching,
    pub error: ErrorInfo,
}

/// The watchdog of `device`, which doesn't power the updating one, couldn't be suspended; the
/// update goes on, and `device` may reboot meanwhile.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogLeft {
    pub device: Box<str>,
    pub error: ErrorInfo,
}

/// A device whose watchdogs may fire while `device` is away.
pub struct Watcher {
    pub device: DeviceAccess,
    /// it powers `device`, directly or further up: losing its watchdog's suspension may brick
    /// `device`, so the update doesn't go on without
    pub supplier: bool,
}

/// The RouterOS devices whose watchdogs may fire while `device` is away: the ones powering it,
/// the ones depending on it on the way to the management roots (but those whose
/// `watchdog_target` names something else) and the ones whose `watchdog_target` is an address
/// of it, nearest suppliers first.
pub fn watchers(device: &DeviceAccess, tree: &ManagementTree) -> Vec<Watcher> {
    let suppliers = super::dependency::supplier_devices(device)
        .into_iter()
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    let ids = suppliers
        .iter()
        .map(DeviceAccess::id)
        .collect::<BTreeSet<_>>();
    let topology = device.topology();
    let own = addresses(device);
    let watching_it = topology
        .list_devices()
        .filter(|other| other.id() != device.id())
        .filter(|other| {
            matches!(other.watchdog_target(),
                WatchdogTarget::Address(address) if own.contains(&address.into()))
        })
        .map(|other| other.id());
    let mut depending = tree
        .depending_on(device.id())
        .into_iter()
        .filter_map(|id| topology.get_device_by_id(&id))
        .filter(|other| {
            matches!(
                other.watchdog_target(),
                WatchdogTarget::Parent | WatchdogTarget::Invalid(_)
            )
        })
        .map(|other| other.id())
        .chain(watching_it)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|id| !ids.contains(id))
        .filter_map(|id| topology.get_device_by_id(&id))
        .filter(DeviceAccess::has_routeros)
        .collect::<Vec<_>>();
    depending.sort_by_cached_key(|device| crate::topology::natural::naturalize(device.name()));
    suppliers
        .into_iter()
        .map(|device| Watcher {
            device,
            supplier: true,
        })
        .chain(depending.into_iter().map(|device| Watcher {
            device,
            supplier: false,
        }))
        .collect()
}

/// Every address of `device` in NetBox: what a port powering it may ping.
pub fn addresses(device: &DeviceAccess) -> Vec<IpAddr> {
    device
        .interfaces()
        .iter()
        .flat_map(|interface| interface.ips())
        .filter_map(|ip| ip.addr())
        .collect()
}

/// What the watchdogs watch, of the ports only those pinging one of `powered` (none read
/// without). While a scheduler entry of a suspend the provisioner didn't see to the end is
/// there, what it restores.
pub async fn read(device: &RawDevice<SimpleResult>, powered: &[IpAddr]) -> Result<Watching, Error> {
    if let Some(watching) = pending(device).await? {
        return Ok(watching);
    }
    read_live(device, powered).await
}

/// What watches now, whatever a scheduler entry would restore.
async fn read_live(
    device: &RawDevice<SimpleResult>,
    powered: &[IpAddr],
) -> Result<Watching, Error> {
    let rows_ = rows(device, "/system/watchdog/print").await?;
    let watch_address = rows_
        .first()
        .and_then(|row| field(row, "watch-address"))
        .filter(|address| !address.is_empty() && *address != "none")
        .map(Box::from);
    let mut poe_pings = Vec::new();
    if !powered.is_empty() {
        for row in rows(device, "/interface/ethernet/poe/print").await? {
            if field(&row, "power-cycle-ping-enabled") != Some("true") {
                continue;
            }
            let (Some(port), Some(address), Some(timeout)) = (
                field(&row, "name"),
                field(&row, "power-cycle-ping-address"),
                field(&row, "power-cycle-ping-timeout"),
            ) else {
                continue;
            };
            if address
                .parse()
                .is_ok_and(|address: IpAddr| powered.contains(&address))
            {
                poe_pings.push(PoePing {
                    port: port.into(),
                    address: address.into(),
                    timeout: timeout.into(),
                });
            }
        }
    }
    Ok(Watching {
        watch_address,
        poe_pings,
    })
}

/// Schedules restoring `watching` after `safety_net`, then suspends it; nothing to do when it
/// is empty.
pub async fn suspend(
    device: &RawDevice<SimpleResult>,
    watching: &Watching,
    safety_net: Duration,
) -> Result<(), Error> {
    if watching.is_empty() {
        return Ok(());
    }
    remove_entry(device).await?;
    command(
        device,
        "/system/scheduler/add",
        &[
            ("name", NAME),
            ("interval", &format!("{}s", safety_net.as_secs().max(1))),
            ("on-event", &script(watching)),
            (
                "comment",
                "restores the watchdogs suspended for an update of a device this one powers or reaches the network through",
            ),
        ],
    )
    .await?;
    if watching.watch_address.is_some() {
        set_watch_address(device, "none").await?;
    }
    for ping in &watching.poe_pings {
        set_poe_ping(device, &ping.port, None).await?;
    }
    Ok(())
}

/// Watches `watching` again and removes the scheduler entry.
pub async fn restore(device: &RawDevice<SimpleResult>, watching: &Watching) -> Result<(), Error> {
    if let Some(address) = &watching.watch_address {
        set_watch_address(device, address).await?;
    }
    for ping in &watching.poe_pings {
        set_poe_ping(device, &ping.port, Some(ping)).await?;
    }
    remove_entry(device).await
}

/// Restores what was suspended and removes its own entry; it checks the entry first, since
/// removing a scheduler entry doesn't stop the script it runs.
fn script(watching: &Watching) -> String {
    let mut commands = Vec::new();
    if let Some(address) = &watching.watch_address {
        commands.push(format!("/system watchdog set watch-address={address}"));
    }
    for PoePing {
        port,
        address,
        timeout,
    } in &watching.poe_pings
    {
        commands.push(format!(
            "/interface ethernet poe set [find name=\"{port}\"] power-cycle-ping-enabled=yes \
             power-cycle-ping-address={address} power-cycle-ping-timeout={timeout}"
        ));
    }
    commands.push(format!("/system scheduler remove [find name=\"{NAME}\"]"));
    format!(
        ":if ([:len [/system scheduler find name=\"{NAME}\"]] > 0) do={{ {} }}",
        commands.join("; ")
    )
}

/// What the script of a scheduler entry left from a suspend restores.
fn restored_by(script: &str) -> Option<Watching> {
    let mut watching = Watching::default();
    for command in script.split("; ") {
        let value = |name: &str| {
            let (_, rest) = command.split_once(&format!("{name}="))?;
            rest.split([' ', '"', ']', '}'])
                .next()
                .filter(|value| !value.is_empty())
                .map(Box::from)
        };
        if command.contains("/system watchdog set") {
            watching.watch_address = value("watch-address");
        } else if let (Some(port), Some(address), Some(timeout)) = (
            command
                .split_once("/interface ethernet poe set [find name=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(port, _)| Box::from(port)),
            value("power-cycle-ping-address"),
            value("power-cycle-ping-timeout"),
        ) {
            watching.poe_pings.push(PoePing {
                port,
                address,
                timeout,
            });
        }
    }
    (!watching.is_empty()).then_some(watching)
}

/// What a scheduler entry left from a suspend would restore, if there is one.
pub(crate) async fn pending(device: &RawDevice<SimpleResult>) -> Result<Option<Watching>, Error> {
    Ok(pending_in(&entries(device).await?))
}

/// [`pending`] from the rows of `/system/scheduler` read already.
pub(crate) fn pending_in(schedulers: &[crate::device::raw::Row]) -> Option<Watching> {
    schedulers
        .iter()
        .filter(|row| field(row, "name") == Some(NAME))
        .find_map(|row| field(row, "on-event").and_then(restored_by))
}

async fn entries(device: &RawDevice<SimpleResult>) -> Result<Vec<crate::device::raw::Row>, Error> {
    Ok(rows(device, "/system/scheduler/print")
        .await?
        .into_iter()
        .filter(|row| field(row, "name") == Some(NAME))
        .collect())
}

async fn remove_entry(device: &RawDevice<SimpleResult>) -> Result<(), Error> {
    for row in entries(device).await? {
        if let Some(id) = field(&row, ".id") {
            command(device, "/system/scheduler/remove", &[(".id", id)]).await?;
        }
    }
    Ok(())
}

async fn set_watch_address(device: &RawDevice<SimpleResult>, address: &str) -> Result<(), Error> {
    command(
        device,
        "/system/watchdog/set",
        &[("watch-address", address)],
    )
    .await?;
    Ok(())
}

/// Switches the power cycling of `port` off, or on as `ping` says.
async fn set_poe_ping(
    device: &RawDevice<SimpleResult>,
    port: &str,
    ping: Option<&PoePing>,
) -> Result<(), Error> {
    let rows_ = rows(device, "/interface/ethernet/poe/print").await?;
    let id = rows_
        .iter()
        .find(|row| field(row, "name") == Some(port))
        .and_then(|row| field(row, ".id"))
        .ok_or_else(|| Error::Refused(format!("/interface/ethernet/poe: no port {port}").into()))?;
    let attributes = match ping {
        None => vec![(".id", id), ("power-cycle-ping-enabled", "no")],
        Some(ping) => vec![
            (".id", id),
            ("power-cycle-ping-enabled", "yes"),
            ("power-cycle-ping-address", &*ping.address),
            ("power-cycle-ping-timeout", &*ping.timeout),
        ],
    };
    command(device, "/interface/ethernet/poe/set", &attributes).await?;
    Ok(())
}

/// The watchdogs suspended now, by device: how many jobs hold them, and what restores them.
static HELD: LazyLock<Mutex<HashMap<Box<str>, Held>>> = LazyLock::new(Default::default);

struct Held {
    holders: u32,
    watching: Watching,
}

/// One job at a time suspends or restores a device's watchdogs: they read and write the same
/// scheduler entry.
static TURNS: LazyLock<Mutex<HashMap<Box<str>, Turn>>> = LazyLock::new(Default::default);

type Turn = Arc<tokio::sync::Mutex<()>>;

fn turn(device: &str) -> Turn {
    TURNS
        .lock_anyway()
        .entry(device.into())
        .or_default()
        .clone()
}

/// What another job holds suspended on `device`.
fn held(device: &str) -> Option<Watching> {
    HELD.lock_anyway()
        .get(device)
        .map(|held| held.watching.clone())
}

/// One more job holds `watching` suspended on `device` (all of it suspended now).
fn join(device: &str, watching: &Watching) {
    let mut held = HELD.lock_anyway();
    let entry = held.entry(device.into()).or_insert(Held {
        holders: 0,
        watching: Watching::default(),
    });
    entry.holders += 1;
    entry.watching = merged(&entry.watching, watching);
}

/// A job lets go of `device`: what to restore when it was the last (or held it unregistered,
/// `own`), else how many still hold it.
fn leave(device: &str, own: &Watching) -> Result<Watching, u32> {
    let mut held = HELD.lock_anyway();
    let Some(entry) = held.get_mut(device) else {
        return Ok(own.clone());
    };
    entry.holders = entry.holders.saturating_sub(1);
    if entry.holders > 0 {
        return Err(entry.holders);
    }
    Ok(held
        .remove(device)
        .map(|held| held.watching)
        .unwrap_or_default())
}

/// `held` with what of `more` it lacks: the watch address one of them knows, every port once.
fn merged(held: &Watching, more: &Watching) -> Watching {
    let mut watching = held.clone();
    if watching.watch_address.is_none() {
        watching.watch_address = more.watch_address.clone();
    }
    for ping in &more.poe_pings {
        if !watching
            .poe_pings
            .iter()
            .any(|known| known.port == ping.port)
        {
            watching.poe_pings.push(ping.clone());
        }
    }
    watching
}

/// Suspends the watchdogs of `device` for one more job: on its own (a scheduler entry left
/// from before counts, [`read`]), or joining the jobs holding them already, with what they
/// don't hold yet (its own ports). What is suspended now.
async fn suspend_held(
    device: &str,
    api: &RawDevice<SimpleResult>,
    powered: &[IpAddr],
) -> Result<Watching, Error> {
    let watching = match held(device) {
        None => {
            let watching = read(api, powered).await?;
            suspend(api, &watching, SAFETY_NET).await?;
            watching
        }
        Some(held) => {
            // what still watches: their address reads `none`, their ports are off
            let live = read_live(api, powered).await?;
            let watching = merged(&held, &live);
            if watching != held {
                suspend(api, &watching, SAFETY_NET).await?;
            }
            watching
        }
    };
    if !watching.is_empty() {
        join(device, &watching);
    }
    Ok(watching)
}

/// A device whose watchdogs an update suspended.
pub struct Suspended<L> {
    pub device: Box<str>,
    pub link: L,
    pub watching: Watching,
}

/// A watcher reached for the update, or why not.
pub struct WatcherLink<L> {
    pub device: Box<str>,
    pub supplier: bool,
    /// the updating device's addresses, for the ports of a supplier
    pub powered: Vec<IpAddr>,
    pub link: Result<L, Error>,
}

/// Suspends the watchdogs of each watcher that has one, before an update. A supplier that
/// can't be reached or refuses stops it, with those suspended so far restored; any other
/// watcher is left as it is (`WatchdogLeft`).
pub async fn suspend_all<L: super::update::DeviceLink>(
    watchers: Vec<WatcherLink<L>>,
    log: &Log,
) -> Result<Vec<Suspended<L>>, Error> {
    let mut suspended = Vec::new();
    for WatcherLink {
        device,
        supplier,
        powered,
        link,
    } in watchers
    {
        let result = async {
            let link = link?;
            let turn = turn(&device);
            let _turn = turn.lock().await;
            let api = link.api().await?;
            let powered: &[IpAddr] = if supplier { &powered } else { &[] };
            let watching = suspend_held(&device, &api, powered).await?;
            Ok::<_, Error>((link, watching))
        }
        .await;
        match result {
            Ok((_, watching)) if watching.is_empty() => log.emit(WatchdogNotSet { device }),
            Ok((link, watching)) => {
                log.emit(WatchdogSuspended {
                    device: device.clone(),
                    watching: watching.clone(),
                    safety_net: SAFETY_NET.into(),
                });
                suspended.push(Suspended {
                    device,
                    link,
                    watching,
                });
            }
            Err(Error::Cancelled) => {
                restore_all(suspended, log).await;
                return Err(Error::Cancelled);
            }
            Err(error) if supplier => {
                restore_all(suspended, log).await;
                return Err(Error::WatchdogNotSuspended {
                    device,
                    cause: error.to_string().into(),
                });
            }
            Err(error) => log.emit(WatchdogLeft {
                device,
                error: (&error).into(),
            }),
        }
    }
    Ok(suspended)
}

/// Restores what [`suspend_all`] suspended, unless another job still holds it; one it can't is
/// left to its scheduler entry.
pub async fn restore_all<L: super::update::DeviceLink>(suspended: Vec<Suspended<L>>, log: &Log) {
    for Suspended {
        device,
        link,
        watching,
    } in suspended
    {
        let turn = turn(&device);
        let _turn = turn.lock().await;
        let watching = match leave(&device, &watching) {
            Ok(watching) => watching,
            Err(others) => {
                log.emit(WatchdogStillHeld { device, others });
                continue;
            }
        };
        match async { restore(&link.api().await?, &watching).await }.await {
            Ok(()) => log.emit(WatchdogRestored { device, watching }),
            Err(error) => log.emit(WatchdogNotRestored {
                device,
                watching,
                error: (&error).into(),
            }),
        }
    }
}

impl<L> Suspended<L> {
    /// What a job saved before a restart holds suspended on `device`, held again: a job
    /// suspending it meanwhile joins it, and restoring waits for the last.
    pub(crate) fn held_again(device: Box<str>, link: L, watching: Watching) -> Self {
        if !watching.is_empty() {
            join(&device, &watching);
        }
        Suspended {
            device,
            link,
            watching,
        }
    }
}

/// The watchdogs of others that may fire while a device updates (`suspendWatchdogs`) or is set
/// up, and the addresses its suppliers' PoE ports ping.
pub(crate) struct Watchers {
    watchers: Vec<Watcher>,
    powered: Vec<IpAddr>,
}

impl Watchers {
    pub(crate) fn of(device: &DeviceAccess, suspend: bool) -> Self {
        Self {
            watchers: match suspend {
                true => watchers(device, &ManagementTree::of_device(device)),
                false => Vec::new(),
            },
            powered: addresses(device),
        }
    }
    /// Only the devices powering `device`, for its setup: a device in factory state doesn't
    /// answer the PoE ping of its addresses in NetBox (one replacing another), and would lose
    /// its power every few minutes, in the middle of its setup too (E2).
    pub(crate) fn suppliers(device: &DeviceAccess) -> Self {
        let mut watchers = Self::of(device, true);
        watchers.watchers.retain(|watcher| watcher.supplier);
        watchers
    }
    /// Connects to each and suspends its watchdogs, the job entering `suspending` first.
    pub(crate) async fn suspend(
        self,
        log: &Log,
        suspending: impl Into<JobPhase>,
    ) -> Result<Vec<Suspended<AccessibleDevice>>, Error> {
        if !self.watchers.is_empty() {
            log.enter(suspending);
        }
        let mut links = Vec::new();
        for Watcher { device, supplier } in self.watchers {
            let link = connect_for_job(&device, DeviceTarget::default(), log).await;
            if matches!(link, Err(Error::Cancelled)) {
                return Err(Error::Cancelled);
            }
            links.push(WatcherLink {
                device: device.name().into(),
                supplier,
                powered: self.powered.clone(),
                link,
            });
        }
        suspend_all(links, log).await
    }
}

/// Sets the watchdogs [`Watchers::suspend`] suspended again, whatever the job came to, the job
/// entering `restoring` first.
pub(crate) async fn restore_watchdogs(
    suspended: Vec<Suspended<AccessibleDevice>>,
    log: &Log,
    restoring: impl Into<JobPhase>,
) {
    if !suspended.is_empty() {
        log.enter(restoring);
    }
    restore_all(suspended, log).await;
}

#[cfg(test)]
mod tests {
    use super::{PoePing, Watching, restored_by, script};

    #[test]
    fn the_script_names_what_it_restores() {
        let watching = Watching {
            watch_address: Some("172.16.1.2".into()),
            poe_pings: vec![PoePing {
                port: "e01-sxt-a".into(),
                address: "192.168.100.4".into(),
                timeout: "5m".into(),
            }],
        };
        assert_eq!(restored_by(&script(&watching)), Some(watching));
        let only_poe = Watching {
            watch_address: None,
            poe_pings: vec![PoePing {
                port: "ether2".into(),
                address: "10.0.0.1".into(),
                timeout: "5m".into(),
            }],
        };
        assert_eq!(restored_by(&script(&only_poe)), Some(only_poe));
        assert_eq!(restored_by("/system reboot"), None);
    }
}
