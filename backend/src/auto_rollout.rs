//! Scope: The automatic rollout (C20, `docs/auto-rollout.md`): when a round runs, what
//! it reads, its state per site, kept in `state_dir`; not what a round applies (`decide`) nor
//! applying (`device::batch`).
//!
//! A round runs after a change in NetBox (the webhook, [`netbox_changed`], once NetBox was
//! quiet for [`QUIET`]), [`EVERY`] 30 min without one (hand-made changes go back within it),
//! after [`RETRY`] when it left something (a device passed over, no internet, a device busy),
//! and at once when an admin clears a site's error or releases a manual device ([`wake`]).
//! It reads the diff of every automatic device and of the manual ones next to them, then
//! applies the automatic devices with changes as one batch that needs the internet before and
//! after (`batch::apply`). A batch that fails puts the sites of its devices into their error
//! state; their devices wait until an admin clears it.
//!
//! Before reading, a round checks the precondition in NetBox (`precondition`), and with the
//! diffs the device mode of each automatic device: unmet, it applies nothing and the next
//! round, after [`RETRY`] or a change in NetBox, checks again.
pub mod decide;
pub mod graphql;
pub mod hold;
pub mod precondition;
pub mod webhook;

use crate::{
    Error,
    config::CONFIG,
    device::{batch, ros::fallback},
    facts::Freshness,
    job::{self, JobKind},
    journal::Started,
    lock::LockAnyway,
    topology::{
        DeviceId, SiteId, Topology, TopologyHolder,
        access::{AccessTopology, DeviceTarget, device::DeviceAccess},
    },
};
use decide::{Diff, Fingerprint, Seen};
use hold::HoldReason;
use log::{info, warn};
use precondition::Unmet;
use prometheus::{IntGauge, IntGaugeVec, Opts, Registry};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::{Notify, Semaphore},
    task::JoinSet,
    time::Instant,
};

/// A round at least this often: what was changed by hand on an automatic device goes back
/// within it.
pub const EVERY: Duration = Duration::from_secs(30 * 60);
/// How long NetBox has to be quiet after a change before a round: an admin changes several
/// objects one after the other, a helper's plan writes many.
pub const QUIET: Duration = Duration::from_secs(20);
/// The next round after one that left something to do: a device passed over, no internet, a
/// device busy with another job.
pub const RETRY: Duration = Duration::from_secs(5 * 60);
/// How many devices a round reads at once: they share the way through the management proxy.
const AT_ONCE: usize = 4;

/// What is kept between rounds, in `state_dir/auto_rollout.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// every site seen automatic
    pub sites: BTreeMap<SiteId, SiteState>,
    /// their names, as NetBox had them at the last round (the metrics' label)
    #[serde(default)]
    pub site_names: BTreeMap<SiteId, Box<str>>,
    /// the diffs of the manual devices next to automatic ones, as the last round saw them
    pub baselines: BTreeMap<DeviceId, Fingerprint>,
    /// the diffs an admin released (of a manual device, or an automatic one with a reason to
    /// wait): the next round applies them with the others, a diff changed since holds again
    #[serde(default)]
    pub released_diffs: BTreeMap<DeviceId, Fingerprint>,
    /// the diffs of the devices holding the last round back, which an admin may release, or take
    /// over (a manual device's)
    /// over as their baselines ([`accept`])
    #[serde(default)]
    pub holding: BTreeMap<DeviceId, Fingerprint>,
    /// the last round
    pub last: Option<RoundDone>,
    /// the automatic devices the precondition failed on at the last round: none apply until
    /// it is met again
    #[serde(default)]
    pub unmet: Box<[Unmet]>,
}

/// A device a round names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, async_graphql::SimpleObject)]
pub struct RoundDevice {
    pub id: u32,
    pub name: Box<str>,
}

impl RoundDevice {
    fn of(device: &DeviceAccess) -> Self {
        RoundDevice {
            id: device.id().0,
            name: device.name().into(),
        }
    }
}

/// An automatic device whose changes wait for an admin, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, async_graphql::SimpleObject)]
pub struct HeldDevice {
    pub device: RoundDevice,
    pub reasons: Box<[HoldReason]>,
}

/// An automatic site, as the rounds see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SiteState {
    Ready,
    /// its devices wait until an admin clears it
    Error {
        reason: SiteError,
        /// the devices it is about
        devices: Box<[RoundDevice]>,
        at: SystemTime,
    },
}

/// Why a site is in its error state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, async_graphql::Enum)]
pub enum SiteError {
    /// switched on with changes on its devices: they would all go out with the first change
    NotInLine,
    /// the batch of a round failed on its devices: every device went back
    ApplyFailed,
}

/// What a round did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundDone {
    pub at: SystemTime,
    pub outcome: RoundOutcome,
    /// the devices it applied (or tried to)
    pub applied: Box<[RoundDevice]>,
    /// automatic devices with changes it passed over: not reached, or their radio partner
    pub deferred: Box<[RoundDevice]>,
    /// manual devices whose diff changed, holding it back
    pub held_by: Box<[RoundDevice]>,
    /// automatic devices whose changes wait for an admin, holding it back
    #[serde(default)]
    pub held: Box<[HeldDevice]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, async_graphql::Enum)]
pub enum RoundOutcome {
    /// every device as NetBox has it, or passed over
    NothingToDo,
    /// a site switched on wasn't as NetBox has it: it is in its error state, nothing else to do
    NotInLine,
    /// the batch went through
    Applied,
    /// a manual device next to them changed: nothing applied until an admin releases it
    HeldBack,
    /// a device didn't reach the internet over IPv4 and IPv6 before: nothing applied
    NoInternet,
    /// a device was busy with another job: nothing applied
    Busy,
    /// the batch failed, all went back: its sites are in their error state
    Failed,
    /// NetBox couldn't be read
    NetboxUnreadable,
    /// the precondition failed on an automatic device: nothing applied (nothing read, where
    /// NetBox broke it)
    PreconditionUnmet,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

/// Per automatic site, 1 while it is in its error state: the alarm (Alertmanager, Z13).
static SITE_ERROR: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    IntGaugeVec::new(
        Opts::new(
            "auto_rollout_site_error",
            "1 while an automatic site is in its error state, its devices waiting for an admin",
        ),
        &["site"],
    )
    .expect("gauge")
});
/// 1 while a manual device holds the rounds back.
static HELD_BACK: LazyLock<IntGauge> = LazyLock::new(|| {
    IntGauge::new(
        "auto_rollout_held_back",
        "1 while a manual device whose diff changed holds the automatic rollout back",
    )
    .expect("gauge")
});
/// The automatic devices with changes the last round passed over.
static DEFERRED: LazyLock<IntGauge> = LazyLock::new(|| {
    IntGauge::new(
        "auto_rollout_deferred_devices",
        "Automatic devices with changes the last round passed over (not reached, or a radio \
         partner not reached)",
    )
    .expect("gauge")
});

/// Registers the rounds' metrics.
pub fn register(registry: &Registry) {
    fn one(registry: &Registry, metric: impl prometheus::core::Collector + 'static) {
        if let Err(error) = registry.register(Box::new(metric)) {
            warn!("Cannot register the automatic rollout's metrics: {error}");
        }
    }
    one(registry, SITE_ERROR.clone());
    one(registry, HELD_BACK.clone());
    one(registry, DEFERRED.clone());
}
/// NetBox changed (the webhook)
static CHANGED: Notify = Notify::const_new();
/// an admin wants a round now
static WAKE: Notify = Notify::const_new();

/// A change in NetBox: a round once it is quiet for [`QUIET`].
pub fn netbox_changed() {
    CHANGED.notify_one();
}

/// A round now (an admin cleared an error or released a device).
pub fn wake() {
    WAKE.notify_one();
}

/// What is kept now.
pub fn state() -> State {
    STATE.lock_anyway().clone()
}

/// Clears the error state of `site`: its devices are applied at the next round.
pub fn clear_error(site: SiteId) -> bool {
    let cleared = {
        let mut state = STATE.lock_anyway();
        match state.sites.get_mut(&site) {
            Some(site @ SiteState::Error { .. }) => {
                *site = SiteState::Ready;
                save(&state);
                true
            }
            _ => false,
        }
    };
    if cleared {
        wake();
    }
    cleared
}

/// Releases the diffs with which devices hold the rounds back: the next applies them too, a
/// diff changed since holds again.
pub fn release(devices: &[DeviceId]) {
    {
        let mut state = STATE.lock_anyway();
        for device in devices {
            if let Some(fingerprint) = state.holding.get(device).cloned() {
                state.released_diffs.insert(*device, fingerprint);
            }
        }
        save(&state);
    }
    wake();
}

/// Takes the diffs with which manual devices hold the rounds back as their baselines: they
/// stay on the devices, and the next round goes on without them. A diff changed since holds
/// back again.
pub fn accept(devices: &[DeviceId]) {
    {
        let mut state = STATE.lock_anyway();
        for device in devices {
            if let Some(fingerprint) = state.holding.remove(device) {
                state.baselines.insert(*device, fingerprint);
            }
        }
        save(&state);
    }
    wake();
}

/// Starts the rounds (with `allow_apply` only), after reading what was kept.
pub fn start(topology: TopologyHolder) {
    if !CONFIG.allow_apply {
        return;
    }
    if let Some(kept) = load() {
        *STATE.lock_anyway() = kept;
    }
    tokio::spawn(async move {
        // the first soon: what changed while the provisioner was away
        let mut next = Instant::now() + QUIET;
        loop {
            tokio::select! {
                _ = CHANGED.notified() => {
                    // until NetBox is quiet
                    while tokio::time::timeout(QUIET, CHANGED.notified()).await.is_ok() {}
                }
                _ = WAKE.notified() => {}
                _ = tokio::time::sleep_until(next) => {}
            }
            let again = match topology.fetch().await {
                Ok(()) => match topology.current().await {
                    Some(current) => round(current).await,
                    None => finish(RoundOutcome::NetboxUnreadable, &[], &[], &[], &[]),
                },
                Err(error) => {
                    warn!("auto rollout: NetBox not read: {error}");
                    finish(RoundOutcome::NetboxUnreadable, &[], &[], &[], &[])
                }
            };
            next = Instant::now() + again;
        }
    });
}

/// One round over `topology`; when the next one is due.
async fn round(topology: Arc<Topology>) -> Duration {
    let (states, baselines, released) = {
        let state = STATE.lock_anyway();
        (
            state.sites.clone(),
            state.baselines.clone(),
            state.released_diffs.clone(),
        )
    };
    let automatic: Vec<DeviceAccess> = topology
        .list_devices()
        .filter(DeviceAccess::rolls_out_automatically)
        .collect();
    let site_names: BTreeMap<SiteId, Box<str>> = automatic
        .iter()
        .filter_map(|device| device.site())
        .map(|site| (site.id(), site.name().into()))
        .collect();
    let unmet = precondition::unmet(&automatic);
    {
        let mut state = STATE.lock_anyway();
        state.unmet = unmet.clone().into();
        save(&state);
    }
    if !unmet.is_empty() {
        let devices = unmet.iter().map(|u| u.device.clone()).collect::<Vec<_>>();
        warn!("auto rollout: precondition unmet on {}", listed(&devices));
        return finish(RoundOutcome::PreconditionUnmet, &[], &[], &[], &[]);
    }
    let sites: BTreeSet<SiteId> = site_names.keys().copied().collect();
    let new_sites: BTreeSet<SiteId> = sites
        .iter()
        .filter(|site| !states.contains_key(site))
        .copied()
        .collect();
    let in_error = |device: &DeviceAccess| {
        device
            .site()
            .and_then(|site| states.get(&site.id()))
            .is_some_and(|state| matches!(state, SiteState::Error { .. }))
    };
    let automatic: Vec<DeviceAccess> = automatic.into_iter().filter(|d| !in_error(d)).collect();
    let mut manual: BTreeMap<DeviceId, DeviceAccess> = BTreeMap::new();
    for device in &automatic {
        for next in related(device) {
            if !next.rolls_out_automatically() {
                manual.insert(next.id(), next);
            }
        }
    }
    let reads = read_diffs(automatic.iter().chain(manual.values()).cloned().collect()).await;
    let without_scheduler: BTreeSet<DeviceId> = reads
        .iter()
        .filter(|(_, read)| read.without_scheduler)
        .map(|(device, _)| *device)
        .collect();
    let unmet = precondition::without_scheduler(&automatic, &without_scheduler);
    if !unmet.is_empty() {
        let devices = unmet.iter().map(|u| u.device.clone()).collect::<Vec<_>>();
        warn!("auto rollout: no scheduler on {}", listed(&devices));
        let mut state = STATE.lock_anyway();
        state.unmet = unmet.into();
        save(&state);
        drop(state);
        return finish(RoundOutcome::PreconditionUnmet, &[], &[], &[], &[]);
    }
    let mut reads = reads;
    let seen: Vec<Seen> = automatic
        .iter()
        .map(|device| (device, device.site().map(|site| site.id())))
        .chain(manual.values().map(|device| (device, None)))
        .map(|(device, site)| {
            let read = reads.remove(&device.id());
            Seen {
                device: device.id(),
                site,
                hold: match site {
                    Some(_) => read.as_ref().map(|r| r.hold.clone()).unwrap_or_default(),
                    None => Box::new([]),
                },
                diff: read.map_or(Diff::Unreached, |read| read.diff),
                partners: batch::wireless_partners(device)
                    .iter()
                    .map(DeviceAccess::id)
                    .collect(),
            }
        })
        .collect();
    let decision = decide::decide(&seen, &baselines, &released, &new_sites);
    let name = |id: &DeviceId| -> RoundDevice {
        topology.get_device_by_id(id).map_or_else(
            || RoundDevice {
                id: id.0,
                name: id.0.to_string().into(),
            },
            |d| RoundDevice::of(&d),
        )
    };
    let names = |ids: &BTreeSet<DeviceId>| ids.iter().map(name).collect::<Vec<_>>();
    {
        let mut state = STATE.lock_anyway();
        state.baselines = decision.baselines.clone();
        state.holding = seen
            .iter()
            .filter(|s| {
                decision.held_by.contains(&s.device) || decision.held.contains_key(&s.device)
            })
            .filter_map(|s| match &s.diff {
                Diff::Changes(fingerprint) => Some((s.device, fingerprint.clone())),
                _ => None,
            })
            .collect();
        state.sites.retain(|site, _| sites.contains(site));
        state.site_names = site_names;
        for site in &new_sites {
            let entry = match decision.not_in_line.contains(site) {
                true => SiteState::Error {
                    reason: SiteError::NotInLine,
                    devices: seen
                        .iter()
                        .filter(|s| s.site == Some(*site) && matches!(s.diff, Diff::Changes(_)))
                        .map(|s| name(&s.device))
                        .collect(),
                    at: SystemTime::now(),
                },
                false => SiteState::Ready,
            };
            state.sites.insert(*site, entry);
        }
        save(&state);
    }
    let deferred = names(&decision.deferred);
    let held_by = names(&decision.held_by);
    let held: Vec<HeldDevice> = decision
        .held
        .iter()
        .map(|(device, reasons)| HeldDevice {
            device: name(device),
            reasons: reasons.clone(),
        })
        .collect();
    if !held_by.is_empty() || !held.is_empty() {
        let devices = held.iter().map(|h| h.device.clone()).chain(held_by.clone());
        info!(
            "auto rollout: held back by {}",
            listed(&devices.collect::<Vec<_>>())
        );
        return finish(RoundOutcome::HeldBack, &[], &deferred, &held_by, &held);
    }
    if decision.members.is_empty() {
        let outcome = match decision.not_in_line.is_empty() {
            true => RoundOutcome::NothingToDo,
            false => RoundOutcome::NotInLine,
        };
        return finish(outcome, &[], &deferred, &[], &[]);
    }
    let members: Vec<DeviceAccess> = decision
        .members
        .iter()
        .filter_map(|id| topology.get_device_by_id(id))
        .collect();
    let applied = names(&decision.members);
    info!("auto rollout: applying {}", listed(&applied));
    // every other automatic device has to answer after the scripts as before
    let bystanders: Vec<DeviceAccess> = topology
        .list_devices()
        .filter(DeviceAccess::rolls_out_automatically)
        .filter(|device| !decision.members.contains(&device.id()))
        .collect();
    let outcome = apply(&members, bystanders).await;
    {
        let mut state = STATE.lock_anyway();
        state
            .released_diffs
            .retain(|id, _| !decision.members.contains(id));
        if let Applied::Failed(failed) = &outcome {
            let failed: Box<[RoundDevice]> = failed.iter().map(RoundDevice::of).collect();
            for site in members.iter().filter_map(|d| d.site()) {
                if state.sites.contains_key(&site.id()) {
                    state.sites.insert(
                        site.id(),
                        SiteState::Error {
                            reason: SiteError::ApplyFailed,
                            devices: failed.clone(),
                            at: SystemTime::now(),
                        },
                    );
                }
            }
        }
        save(&state);
    }
    let outcome = match outcome {
        Applied::Done => RoundOutcome::Applied,
        Applied::NoInternet => RoundOutcome::NoInternet,
        Applied::Busy => RoundOutcome::Busy,
        Applied::Failed(_) => RoundOutcome::Failed,
    };
    finish(outcome, &applied, &deferred, &[], &[])
}

/// How a round's batch ended.
enum Applied {
    Done,
    NoInternet,
    Busy,
    /// these devices failed (cancelled too); all went back
    Failed(Vec<DeviceAccess>),
}

/// Applies `members` as one batch needing the internet and `bystanders` answering after it as
/// before, and waits for its end.
async fn apply(members: &[DeviceAccess], bystanders: Vec<DeviceAccess>) -> Applied {
    let (sent, outcome) = tokio::sync::oneshot::channel();
    let devices = members.to_vec();
    let started = job::start_all(
        members,
        JobKind::Apply,
        |_| Started::Automatic,
        |logs| async move {
            let results = batch::apply(devices.clone(), logs, true, bystanders).await;
            let failed: Vec<DeviceAccess> = devices
                .iter()
                .zip(&results)
                .filter(|(_, result)| result.is_err())
                .map(|(device, _)| device.clone())
                .collect();
            let no_internet = results
                .iter()
                .any(|result| matches!(result, Err(Error::NoInternet(_))));
            let _ = sent.send(match (failed.is_empty(), no_internet) {
                (true, _) => Applied::Done,
                (false, true) => Applied::NoInternet,
                (false, false) => Applied::Failed(failed),
            });
            results
        },
    );
    match started {
        Ok(_) => outcome.await.unwrap_or(Applied::Failed(members.to_vec())),
        Err(error) => {
            info!("auto rollout: not started: {error}");
            Applied::Busy
        }
    }
}

/// Keeps what the round did; when the next is due.
fn finish(
    outcome: RoundOutcome,
    applied: &[RoundDevice],
    deferred: &[RoundDevice],
    held_by: &[RoundDevice],
    held: &[HeldDevice],
) -> Duration {
    let mut state = STATE.lock_anyway();
    state.last = Some(RoundDone {
        at: SystemTime::now(),
        outcome,
        applied: applied.into(),
        deferred: deferred.into(),
        held_by: held_by.into(),
        held: held.into(),
    });
    save(&state);
    let again = !deferred.is_empty()
        || matches!(
            outcome,
            RoundOutcome::NoInternet
                | RoundOutcome::Busy
                | RoundOutcome::NetboxUnreadable
                | RoundOutcome::PreconditionUnmet
        );
    if again { RETRY } else { EVERY }
}

fn listed(devices: &[RoundDevice]) -> String {
    devices
        .iter()
        .map(|device| &*device.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a round read of one device.
struct Read {
    diff: Diff,
    /// its device mode has the scheduler off
    without_scheduler: bool,
    /// why its changes would wait for an admin (`hold::reasons`)
    hold: Box<[HoldReason]>,
}

/// The diff of each of `devices` and whether it has the scheduler, [`AT_ONCE`] at a time.
async fn read_diffs(devices: Vec<DeviceAccess>) -> BTreeMap<DeviceId, Read> {
    let started = Instant::now();
    let permits = Arc::new(Semaphore::new(AT_ONCE));
    let mut reads = JoinSet::new();
    for device in devices {
        let permits = permits.clone();
        reads.spawn(async move {
            let _permit = permits.acquire_owned().await;
            let read = job::run(diff_of(device.clone(), started))
                .await
                .unwrap_or(Read {
                    diff: Diff::Unreached,
                    without_scheduler: false,
                    hold: Box::new([]),
                });
            (device.id(), read)
        });
    }
    let mut diffs = BTreeMap::new();
    while let Some(read) = reads.join_next().await {
        if let Ok((device, diff)) = read {
            diffs.insert(device, diff);
        }
    }
    diffs
}

/// Read anew for the round; a device not reached, or whose config can't be generated, is
/// [`Diff::Unreached`].
async fn diff_of(device: DeviceAccess, started: Instant) -> Result<Read, Error> {
    let Some(link) = device.connect(DeviceTarget::default()).await? else {
        return Ok(Read {
            diff: Diff::Unreached,
            without_scheduler: false,
            hold: Box::new([]),
        });
    };
    let generated = link.generated_cfg(Freshness::Since(started)).await?;
    let reader = link.reader().at(Freshness::Since(started));
    let without_scheduler = fallback::without_scheduler(&reader).await?.is_some();
    Ok(Read {
        diff: match generated.fingerprint() {
            Some(fingerprint) => Diff::Changes(fingerprint),
            None => Diff::InLine,
        },
        without_scheduler,
        hold: hold::reasons(
            &device,
            generated.apply_warnings(),
            generated.changes_radio(),
        ),
    })
}

/// The RouterOS devices a change to `device` may need changed too: at the other end of its
/// cables and radio links, in its WLAN group, and on its VXLANs.
fn related(device: &DeviceAccess) -> Vec<DeviceAccess> {
    let mut found: BTreeMap<DeviceId, DeviceAccess> = BTreeMap::new();
    for port in device.interfaces() {
        for far in port.connected_interfaces() {
            if let Some(other) = far.device() {
                found.insert(other.id(), other);
            }
        }
    }
    for other in batch::wireless_partners(device) {
        found.insert(other.id(), other);
    }
    for group in [device.wlan_controller_of(), device.wlan_ap_of()]
        .into_iter()
        .flatten()
    {
        found.extend(group.controller().map(|c| (c.id(), c)));
        found.extend(group.aps().iter().map(|ap| (ap.id(), ap.clone())));
    }
    let topology = device.topology();
    for vlan in topology.list_vlans().filter(|vlan| vlan.tunnel().is_some()) {
        let devices = vlan.devices();
        if devices.iter().any(|d| d.id() == device.id()) {
            found.extend(devices.iter().map(|d| (d.id(), d.clone())));
        }
    }
    found.remove(&device.id());
    found
        .into_values()
        .filter(DeviceAccess::has_routeros)
        .collect()
}

fn file() -> Option<PathBuf> {
    crate::saved_job::state_file("auto_rollout.json")
}

fn load() -> Option<State> {
    let path = file()?;
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|error| warn!("{} not read: {error}", path.display()))
        .ok()
}

/// Writes `state` (a temporary file, then renamed; a failure only goes to the log) and sets
/// the metrics from it.
fn save(state: &State) {
    SITE_ERROR.reset();
    for (site, site_state) in &state.sites {
        let error = matches!(site_state, SiteState::Error { .. });
        let name = state.site_names.get(site).map_or("", |name| name);
        SITE_ERROR.with_label_values(&[name]).set(error.into());
    }
    if let Some(last) = &state.last {
        HELD_BACK.set((last.outcome == RoundOutcome::HeldBack).into());
        DEFERRED.set(last.deferred.len().try_into().unwrap_or(i64::MAX));
    }
    crate::facts::announce(crate::facts::Change::AutoRollout);
    let Some(path) = file() else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    let written = std::fs::File::create(&temporary).and_then(|file| {
        serde_json::to_writer_pretty(&file, state)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)
    });
    if let Err(error) = written {
        warn!("Cannot save the automatic rollout's state: {error}");
    }
}
