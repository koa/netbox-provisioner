//! Applying the config to several devices together, with one fallback for all of them: the
//! two ends of a radio link only work again with both changed, and the far one is often only
//! reached across the link. A single apply would take the far end's fallback away once it is
//! reached, or leave the near end changed while the far end goes back.
//!
//! Step by step, each step on all devices at once, the next only once all are through:
//! planning, then the fallback and the script on each device ([`fallback_delay`]: all firing
//! at one moment), then starting the scripts. Each script first waits [`LEAD`], time enough
//! to start all the others before the first changes cut a connection (started one after the
//! other without waiting, the first end of the 60 GHz link cut the way to the second before
//! its script could start); one that doesn't start leaves its device not in line, so all go
//! back (started scripts aren't stopped). Only once every device is
//! reached again and in line are the fallbacks taken away; if one isn't, none is, and all go
//! back to the config from before together (the operator asked for that: a half changed link
//! can't be repaired from afar; reached over another way but not in line counts as not
//! through, lab-w60-b was). A problem before the scripts start stops the batch with nothing
//! applied; cancelling any of its jobs cancels all.
//!
//! A device whose plan changes a radio suspends its ping watchdog and those depending on it
//! before the fallbacks (`radio_hold`), and restores them once all are through or nothing is
//! applied; while the fallbacks may still fire they stay suspended.
use crate::{
    Error,
    config::CONFIG,
    device_state::{ApplyState, JobPhase},
    error_info::ErrorInfo,
    device::{
        AccessibleDevice, bootstrap,
        radio_hold::RadioHold,
        ros::{
            fallback::{Pending, Rearmed},
            progress::Progress,
            graphql::{
                APPLY_TIMEOUT, ApplyStarted, FALLBACK_MARGIN, FallbackLeftReason, PlannedApply,
                SCRIPT_LIMIT, ScriptProgress, arm_fallback_on, emit_checked, fallback_left,
                fallback_window, look_on, remove_script, script_running, start_script,
                write_script,
            },
        },
    },
    graphql::connect_for_job,
    job::Log,
    saved_job::{self, SavedApply, SavedBatch, SavedJob, SavedWork, ScriptRun},
    topology::{DeviceId, access::{AccessTopology, device::DeviceAccess}},
};
use async_graphql::{SimpleObject, futures_util::future::join_all};
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use std::time::Instant;
use tokio::time::sleep;

/// The devices of the batch.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct BatchApply {
    pub devices: Vec<Box<str>>,
}

/// The batch stopped before any script started: `device` failed (its own job says why); the
/// fallbacks and scripts set by then are taken away.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct BatchStopped {
    pub device: Box<str>,
}

/// After the scripts, `devices` weren't reached and `not_applied` not in line: no fallback is
/// taken away, every device of the batch loads its config from before.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct BatchIncomplete {
    pub devices: Vec<Box<str>>,
    pub not_applied: Vec<Box<str>>,
}

/// How long setting up the fallback and leaving the script on a device may take.
const PREPARE_TIME: Duration = Duration::from_secs(40);
/// How long each script waits once started, before it applies: starting all and seeing them
/// run takes less.
const LEAD: Duration = Duration::from_secs(15);
/// How long starting a script and seeing it run may take, within [`LEAD`].
const START_TIME: Duration = Duration::from_secs(10);

/// The fallback delay of a batch: the single apply's after the scripts start; longer by the
/// time a radio takes to come back when one of them changes one. Each script brings its
/// fallback forward to the window at its end.
fn fallback_delay() -> Duration {
    SCRIPT_LIMIT + CONFIG.apply_fallback() + PREPARE_TIME + LEAD
}

/// What the batch does with a device once its script runs ([`watch`]): one it started
/// ([`Linked`]), one going on after a restart ([`Rejoined`]) or, in the tests, a stand-in.
pub(crate) trait Watched: Send + Sync {
    /// Restores what [`Applying::hold`] suspended once the fallback won't fire (`back`), else
    /// leaves it to the scheduler entries.
    fn release(&self, back: bool, log: &Log) -> impl Future<Output = ()> + Send;
    /// What is left once the script is through (`ScriptProgress`) and the device in line, or
    /// once `settled` passed; `None` while not, or not reached.
    fn check(&self, settled: Instant) -> impl Future<Output = Option<Result<Box<str>, Error>>> + Send;
    /// Where the fallback stood at the last check that reached the device, and when that was.
    fn pending(&self) -> Option<(Pending, Instant)>;
    /// A last look at a device [`Self::check`] never found in line.
    fn last_look(&self) -> impl Future<Output = Result<Box<str>, Error>> + Send;
    /// Takes the fallback away (`FallbackStopped`, or `FallbackLeft`).
    fn stop(&self, fires: Instant, delay: Duration, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
    /// What follows the apply (a controller provisions its CAPs' radios again).
    fn after(&self, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
}

/// The steps of one device's apply, as the batch runs them ([`run`]); a device reached over
/// the provisioner's connection ([`Linked`]) or, in the tests, a stand-in.
pub(crate) trait Applying: Watched {
    /// Connects and plans the diff (`ApplyPlanned`); `false` when there is nothing to do.
    fn plan(&mut self, log: &Log) -> impl Future<Output = Result<bool, Error>> + Send;
    /// How much longer the fallback waits for a radio the plan changes, if it does.
    fn radio_wait(&self) -> Option<Duration>;
    /// Suspends the watchdogs a radio the plan changes may set off (`radio_hold`); nothing
    /// without one.
    fn hold(&mut self, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
    /// Sets up the fallback, firing in `delay`, `radio_wait` of it for a radio, or `window`
    /// after the script's end (`FallbackArmed`); `false` where the device mode doesn't allow
    /// one (`FallbackUnavailable`).
    fn arm(
        &self,
        delay: Duration,
        window: Duration,
        radio_wait: Option<Duration>,
        log: &Log,
    ) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Leaves the script on the device, applying `lead` after it starts and bringing the
    /// fallback forward to `rearm` at its end (`ApplyWritten`).
    fn write(
        &self,
        lead: Duration,
        rearm: Option<Duration>,
        log: &Log,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Starts the script; `true` once it runs.
    fn start(&self) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Whether the started script runs (by now).
    fn running(&self) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Takes the script away before it is started (also one never written).
    fn remove(&self) -> impl Future<Output = Result<(), Error>> + Send;
    /// Hands what going on after a restart needs to `log` ([`Log::keep`]), once the script
    /// is started: with its fallback (`fallback`) firing at `fires`, in `batch`.
    fn keep(&self, _fallback: bool, _fires: Instant, _batch: &SavedBatch, _log: &Log) {}
}

/// A device of the batch, reached the way every job reaches it.
pub(crate) struct Linked {
    device: DeviceAccess,
    reached: Option<Reached>,
    /// the devices of the batch, which hold their own watchdogs
    batch: Arc<BTreeSet<DeviceId>>,
    /// the devices whose watchdogs a member holds already
    claimed: Arc<Mutex<BTreeSet<DeviceId>>>,
}

/// A planned device of the batch: its connection, the plan, and a second connection for the
/// steps up to starting the script, opened with the first (no login between the steps).
struct Reached {
    link: AccessibleDevice,
    planned: PlannedApply,
    control: RawDevice<SimpleResult>,
    hold: Mutex<Option<RadioHold>>,
    /// the run of the script, to read its marks
    run: Mutex<Option<Box<str>>>,
    /// what the last check found
    seen: Mutex<Seen>,
    /// the device's job, for the marks found by a check
    log: Log,
}

#[derive(Default)]
struct Seen {
    pending: Option<(Pending, Instant)>,
    /// where the script brought the fallback forward
    rearmed: Option<Rearmed>,
    /// `ScriptProgress` emitted
    shown: bool,
}

impl Linked {
    fn reached(&self) -> &Reached {
        self.reached.as_ref().expect("planned before")
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

impl Applying for Linked {
    async fn plan(&mut self, log: &Log) -> Result<bool, Error> {
        let link = connect_for_job(&self.device, None, None, None, log).await?;
        let planned = link.plan_apply(log).await?;
        let something = !planned.is_empty();
        if something {
            let control = link.connect_again::<SimpleResult>().await?;
            self.reached = Some(Reached {
                link,
                planned,
                control,
                hold: Mutex::new(None),
                run: Mutex::new(None),
                seen: Mutex::new(Seen::default()),
                log: log.clone(),
            });
        }
        Ok(something)
    }
    fn radio_wait(&self) -> Option<Duration> {
        self.reached().planned.radio_wait()
    }
    async fn hold(&mut self, log: &Log) -> Result<(), Error> {
        let (batch, claimed) = (self.batch.clone(), self.claimed.clone());
        let reached = self.reached.as_mut().expect("planned before");
        if reached.planned.radio_wait().is_some() {
            let hold = RadioHold::take(&reached.link, &mut reached.planned, &batch, &claimed, log).await?;
            *reached.hold.lock().unwrap_or_else(|p| p.into_inner()) = Some(hold);
        }
        Ok(())
    }
    async fn arm(
        &self,
        delay: Duration,
        window: Duration,
        radio_wait: Option<Duration>,
        log: &Log,
    ) -> Result<bool, Error> {
        arm_fallback_on(&self.reached().control, delay, window, radio_wait, log).await
    }
    async fn write(&self, lead: Duration, rearm: Option<Duration>, log: &Log) -> Result<(), Error> {
        let reached = self.reached();
        let run = write_script(&reached.control, &reached.planned, lead, rearm, Some(log)).await?;
        *lock(&reached.run) = Some(run);
        Ok(())
    }
    async fn start(&self) -> Result<bool, Error> {
        start_script(&self.reached().control).await
    }
    async fn running(&self) -> Result<bool, Error> {
        script_running(&self.reached().control).await
    }
    async fn remove(&self) -> Result<(), Error> {
        remove_script(&self.reached().control).await
    }
    fn keep(&self, fallback: bool, fires: Instant, batch: &SavedBatch, log: &Log) {
        let reached = self.reached();
        let (Some(run), Some(credentials)) = (lock(&reached.run).clone(), reached.link.credentials_name()) else {
            return;
        };
        let now = SystemTime::now();
        log.keep(SavedJob {
            device: self.device.id(),
            device_name: self.device.name().into(),
            site: self.device.site().map(|site| site.id()),
            saved_at: now,
            journal_entry: None,
            work: SavedWork::Apply {
                state: ApplyState::ScriptRunning,
                apply: Box::new(SavedApply {
                    credentials: Some(credentials.into()),
                    script: ScriptRun {
                        run,
                        armed: fallback,
                        armed_at: saved_job::system_time(fires.checked_sub(batch.delay).unwrap_or(fires)),
                        fires: saved_job::system_time(fires),
                        unmarked: now + APPLY_TIMEOUT,
                        reprovision: reached.planned.reprovisioned(),
                    },
                    hold: lock(&reached.hold).as_ref().map(RadioHold::saved),
                    batch: Some(batch.clone()),
                }),
            },
        });
    }
}

impl Watched for Linked {
    async fn release(&self, back: bool, log: &Log) {
        let reached = self.reached();
        let hold = lock(&reached.hold).take();
        match hold {
            Some(hold) if back => hold.release(&reached.link, log).await,
            Some(hold) => hold.leave(log),
            None => {}
        }
    }
    async fn check(&self, settled: Instant) -> Option<Result<Box<str>, Error>> {
        let reached = self.reached();
        let run = lock(&reached.run).clone()?;
        check_on(&reached.link, &run, &reached.seen, &reached.log, settled).await
    }
    fn pending(&self) -> Option<(Pending, Instant)> {
        lock(&self.reached().seen).pending
    }
    async fn last_look(&self) -> Result<Box<str>, Error> {
        let reached = self.reached();
        let run = lock(&reached.run).clone();
        last_look_on(&reached.link, run.as_deref(), &reached.seen, &reached.log).await
    }
    async fn stop(&self, fires: Instant, delay: Duration, log: &Log) -> Result<(), Error> {
        let rearmed = lock(&self.reached().seen).rearmed;
        self.reached().link.stop_fallback(fires, delay, rearmed, log).await
    }
    async fn after(&self, log: &Log) -> Result<(), Error> {
        let reached = self.reached();
        reached.link.after_apply(&reached.planned, log).await
    }
}

/// [`Watched::check`] of a device reached at `link`, its script's marks under `run`.
async fn check_on(
    link: &AccessibleDevice,
    run: &str,
    seen: &Mutex<Seen>,
    log: &Log,
    settled: Instant,
) -> Option<Result<Box<str>, Error>> {
    let look = tokio::time::timeout(Duration::from_secs(25), async {
        look_on(&link.connect_again::<SimpleResult>().await?, run).await
    })
    .await
    .ok()?
    .ok()?;
    {
        let mut seen = lock(seen);
        seen.pending = Some((look.pending, Instant::now()));
        if let Some(rearmed) = look.progress.as_ref().and_then(Progress::rearm) {
            seen.rearmed = Some(rearmed);
        }
    }
    // without marks (a file the device couldn't write) through once settled, as before them
    let through = match &look.progress {
        Some(progress) => progress.finished(),
        None => Instant::now() >= settled,
    };
    if !through {
        return None;
    }
    show(seen, log, look.progress.as_ref(), false);
    link.check_applied(settled).await
}

/// [`Watched::last_look`] of a device reached at `link`.
async fn last_look_on(link: &AccessibleDevice, run: Option<&str>, seen: &Mutex<Seen>, log: &Log) -> Result<Box<str>, Error> {
    if let Some(run) = run
        && let Ok(raw) = link.connect_again::<SimpleResult>().await
        && let Ok(look) = look_on(&raw, run).await
    {
        show(seen, log, look.progress.as_ref(), look.pending == Pending::Gone);
    }
    link.remaining_diff().await
}

/// The marks of the script (`ScriptProgress`), once.
fn show(seen: &Mutex<Seen>, log: &Log, progress: Option<&Progress>, after_fallback: bool) {
    let mut seen = lock(seen);
    if let Some(progress) = progress
        && !seen.shown
    {
        seen.shown = true;
        log.emit(ScriptProgress::of(progress, after_fallback));
    }
}

/// A device of a batch saved before a restart (`saved_job`): reached again once it answers,
/// its watchdogs held again then.
pub(crate) struct Rejoined {
    device: DeviceAccess,
    saved: SavedApply,
    link: tokio::sync::Mutex<Option<Arc<AccessibleDevice>>>,
    hold: Mutex<Option<RadioHold>>,
    seen: Mutex<Seen>,
    log: Log,
}

impl Rejoined {
    /// The connection, made now if there is none yet; quiet while the device doesn't answer
    /// (a batch looks every few seconds).
    async fn link(&self) -> Option<Arc<AccessibleDevice>> {
        let mut link = self.link.lock().await;
        if link.is_none() {
            let reached = self.device.connect(None, self.saved.credentials.clone(), None).await.ok().flatten()?;
            self.log.emit(reached.connected_device());
            if let Some(saved) = &self.saved.hold {
                *lock(&self.hold) = Some(RadioHold::resumed(&reached, saved, &self.log).await);
            }
            *link = Some(Arc::new(reached));
        }
        link.clone()
    }
}

impl Watched for Rejoined {
    async fn release(&self, back: bool, log: &Log) {
        let link = self.link.lock().await.clone();
        let hold = lock(&self.hold).take();
        match (hold, link) {
            (Some(hold), Some(link)) if back => hold.release(&link, log).await,
            (Some(hold), _) => hold.leave(log),
            // never reached: what it held stays for the scheduler entries
            (None, None) => {
                if let Some(saved) = &self.saved.hold {
                    RadioHold::leave_saved(self.device.name(), saved, log);
                }
            }
            (None, Some(_)) => {}
        }
    }
    async fn check(&self, settled: Instant) -> Option<Result<Box<str>, Error>> {
        let link = self.link().await?;
        check_on(&link, &self.saved.script.run, &self.seen, &self.log, settled).await
    }
    fn pending(&self) -> Option<(Pending, Instant)> {
        lock(&self.seen).pending
    }
    async fn last_look(&self) -> Result<Box<str>, Error> {
        match self.link().await {
            Some(link) => last_look_on(&link, Some(&self.saved.script.run), &self.seen, &self.log).await,
            None => Err(Error::Timeout(
                bootstrap::reachable_address(&self.device).unwrap_or(IpAddr::from([0u8; 4])),
            )),
        }
    }
    async fn stop(&self, fires: Instant, delay: Duration, log: &Log) -> Result<(), Error> {
        let rearmed = lock(&self.seen).rearmed;
        match self.link().await {
            Some(link) => link.stop_fallback(fires, delay, rearmed, log).await,
            None => {
                fallback_left(log, fires, FallbackLeftReason::NotReached, None);
                Ok(())
            }
        }
    }
    async fn after(&self, log: &Log) -> Result<(), Error> {
        match self.link().await {
            Some(link) => link.after_script(&self.saved.script.reprovision, log).await,
            None => Ok(()),
        }
    }
}

/// The times of a batch: the fallback's delay and its window after a script's end, how long a
/// script waits once started and how long starting it may take, how often and how long it
/// looks for the devices, and how long before the fallback fires it stops.
pub(crate) struct Timing {
    pub delay: Duration,
    pub window: Duration,
    pub lead: Duration,
    pub start: Duration,
    pub poll: Duration,
    pub settle: Duration,
    pub margin: Duration,
}

/// Applies the config to `devices` together, each emitting to its log in `logs`; each job's
/// outcome, in the order of `devices`.
pub async fn apply(devices: Vec<DeviceAccess>, logs: Vec<Log>) -> Vec<Result<(), Error>> {
    let names: Vec<Box<str>> = devices.iter().map(|d| d.name().into()).collect();
    let ids: Vec<DeviceId> = devices.iter().map(DeviceAccess::id).collect();
    let batch = Arc::new(ids.iter().copied().collect::<BTreeSet<_>>());
    let claimed = Arc::new(Mutex::new(BTreeSet::new()));
    let members = devices
        .into_iter()
        .map(|device| Linked {
            device,
            reached: None,
            batch: batch.clone(),
            claimed: claimed.clone(),
        })
        .collect();
    run(members, ids, names, logs, batch_timing()).await
}

fn batch_timing() -> Timing {
    Timing {
        delay: fallback_delay(),
        window: fallback_window(None),
        lead: LEAD,
        start: START_TIME,
        poll: Duration::from_secs(3),
        settle: APPLY_TIMEOUT,
        margin: FALLBACK_MARGIN,
    }
}

/// One member while the batch runs.
pub(crate) struct Running<M> {
    index: usize,
    name: Box<str>,
    log: Log,
    member: M,
    /// whether it has a fallback: not where the device mode has the scheduler off
    fallback: bool,
    /// what the check after the scripts found: `None` while the device isn't in line, or
    /// can't be reached
    checked: Option<Result<Box<str>, Error>>,
}

/// The first failure of `results` (by member), if any.
fn first_failure(results: Vec<Result<(), Error>>) -> Option<(usize, Error)> {
    results.into_iter().enumerate().find_map(|(i, r)| r.err().map(|e| (i, e)))
}

/// The batch over `members` (named `names`, logging to `logs`).
pub(crate) async fn run<M: Applying>(
    members: Vec<M>,
    devices: Vec<DeviceId>,
    names: Vec<Box<str>>,
    logs: Vec<Log>,
    timing: Timing,
) -> Vec<Result<(), Error>> {
    let count = members.len();
    let event = BatchApply {
        devices: names.clone(),
    };
    logs.iter().for_each(|log| log.emit(event.clone()));
    let mut results: Vec<Option<Result<(), Error>>> = (0..count).map(|_| None).collect();
    // what stopped the batch before the scripts: the device's own error, the others' reason
    let stop = |results: &mut Vec<Option<Result<(), Error>>>, failed: usize, error: Error| {
        for (i, log) in logs.iter().enumerate() {
            if i != failed {
                log.emit(BatchStopped {
                    device: names[failed].clone(),
                });
            }
        }
        cancel_all(&logs, &error);
        let cancelled = matches!(error, Error::Cancelled);
        results[failed] = Some(Err(error));
        for (i, result) in results.iter_mut().enumerate() {
            if i != failed && result.is_none() {
                *result = Some(Err(if cancelled {
                    Error::Cancelled
                } else {
                    Error::BatchStopped(names[failed].clone())
                }));
            }
        }
    };
    let finish = |results: Vec<Option<Result<(), Error>>>| {
        results.into_iter().map(|r| r.unwrap_or(Ok(()))).collect::<Vec<_>>()
    };

    // connect and plan, all at once
    let mut members = members;
    let planned = join_all(members.iter_mut().zip(&logs).map(|(member, log)| async move {
        log.check()?;
        log.enter(ApplyState::Planning);
        member.plan(log).await
    }))
    .await;
    let mut running = Vec::new();
    let mut failure = None;
    let mut failed_too = Vec::new();
    for (index, (member, planned)) in members.into_iter().zip(planned).enumerate() {
        match planned {
            Ok(false) => results[index] = Some(Ok(())),
            Ok(true) => running.push(Running {
                index,
                name: names[index].clone(),
                log: logs[index].clone(),
                member,
                fallback: true,
                checked: None,
            }),
            Err(error) if failure.is_none() => failure = Some((index, error)),
            // its own reason, not the first one's
            Err(error) => failed_too.push((index, error)),
        }
    }
    if let Some((index, error)) = failure {
        for (index, error) in failed_too {
            results[index] = Some(Err(error));
        }
        stop(&mut results, index, error);
        return finish(results);
    }
    if running.is_empty() {
        return finish(results);
    }

    // the watchdogs a changed radio may set off, one device after the other
    for index in 0..running.len() {
        let m = &mut running[index];
        let held = match cancelled(&logs) {
            Ok(()) => {
                if m.member.radio_wait().is_some() {
                    m.log.enter(ApplyState::SuspendingWatchdogs);
                }
                m.member.hold(&m.log).await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = held {
            let failed = m.index;
            release_all(&running, true).await;
            stop(&mut results, failed, error);
            return finish(results);
        }
    }

    // on each a fallback, all firing together, and the script; a radio changed on one of
    // them may keep the others off as long
    let radio_wait = running.iter().filter_map(|m| m.member.radio_wait()).max();
    let delay = timing.delay + radio_wait.unwrap_or_default();
    let window = timing.window + radio_wait.unwrap_or_default();
    let fires = Instant::now() + delay;
    let prepared = join_all(running.iter().map(|m| async {
        let armed = async {
            cancelled(&logs)?;
            m.log.enter(ApplyState::ArmingFallback);
            m.member
                .arm(fires.saturating_duration_since(Instant::now()), window, radio_wait, &m.log)
                .await
        }
        .await;
        match armed {
            Ok(fallback) => {
                let rearm = fallback.then_some(window);
                m.log.enter(ApplyState::WritingScript);
                (Some(fallback), m.member.write(timing.lead, rearm, &m.log).await)
            }
            Err(error) => (None, Err(error)),
        }
    }))
    .await;
    for (m, (fallback, _)) in running.iter_mut().zip(&prepared) {
        m.fallback = fallback.unwrap_or(false);
    }
    let armed: Vec<bool> = running.iter().map(|m| m.fallback).collect();
    if let Some((failed, error)) = first_failure(prepared.into_iter().map(|(_, r)| r).collect()) {
        // nothing applied yet: taking it away is all there is to do
        take_back(&running, &armed, fires, delay).await;
        release_all(&running, true).await;
        stop(&mut results, running[failed].index, error);
        return finish(results);
    }

    // start them all at once; each waits `lead` before it applies, one not started stays as
    // it is and the check below sends all back
    if let Err(error) = cancelled(&logs) {
        take_back(&running, &armed, fires, delay).await;
        release_all(&running, true).await;
        stop(&mut results, running[0].index, error);
        return finish(results);
    }
    let start = Instant::now();
    let saved_batch = SavedBatch {
        devices: devices.clone(),
        delay,
    };
    let saved_batch = &saved_batch;
    join_all(running.iter().map(|m| async move {
        m.log.enter(ApplyState::StartingScript);
        let started = tokio::time::timeout(timing.start, async {
            let mut runs = m.member.start().await?;
            while !runs {
                sleep(Duration::from_millis(500)).await;
                runs = m.member.running().await?;
            }
            Ok::<_, Error>(())
        })
        .await
        .unwrap_or(Err(Error::ScriptNotStarted(timing.start)));
        m.log.emit(ApplyStarted {
            starts_in: timing.lead.saturating_sub(start.elapsed()).into(),
            error: started.err().map(|error| ErrorInfo::from(&error)),
        });
        m.member.keep(m.fallback, fires, saved_batch, &m.log);
        m.log.enter(ApplyState::ScriptRunning);
    }))
    .await;
    // from here on the scripts apply without the provisioner: cancelled, the fallbacks stay
    sleep(timing.lead.saturating_sub(start.elapsed())).await;
    watch(running, &logs, &timing, fires, delay, results).await
}

/// Goes on with a batch saved before a restart of the provisioner (`saved_job`): each of
/// `members` with its saved apply and its job's log, the fallbacks firing at `fires` after
/// `delay`. As after the start of the scripts: all through and in line, the fallbacks go,
/// else all go back.
pub async fn resume(members: Vec<(DeviceAccess, SavedApply)>, logs: Vec<Log>, fires: Instant, delay: Duration) -> Vec<Result<(), Error>> {
    let running = members
        .into_iter()
        .zip(&logs)
        .enumerate()
        .map(|(index, ((device, saved), log))| Running {
            index,
            name: device.name().into(),
            log: log.clone(),
            fallback: saved.script.armed,
            member: Rejoined {
                device,
                saved,
                link: Default::default(),
                hold: Mutex::new(None),
                seen: Mutex::new(Seen::default()),
                log: log.clone(),
            },
            checked: None,
        })
        .collect();
    let results = logs.iter().map(|_| None).collect();
    watch(running, &logs, &batch_timing(), fires, delay, results).await
}

/// The batch once its scripts run: waits until every device is reached and checked, then
/// takes all fallbacks away, or leaves all.
pub(crate) async fn watch<M: Watched>(
    mut running: Vec<Running<M>>,
    logs: &[Log],
    timing: &Timing,
    fires: Instant,
    delay: Duration,
    mut results: Vec<Option<Result<(), Error>>>,
) -> Vec<Result<(), Error>> {
    let finish = |results: Vec<Option<Result<(), Error>>>| {
        results.into_iter().map(|r| r.unwrap_or(Ok(()))).collect::<Vec<_>>()
    };
    // until every device is reached and checked, or the fallbacks are about to fire: the
    // latest, or the earliest a device reached said, as each script brings its own forward
    let settled = Instant::now() + timing.settle;
    let latest = fires.checked_sub(timing.margin).unwrap_or(fires);
    loop {
        sleep(timing.poll).await;
        if cancelled(logs).is_err() {
            release_all(&running, false).await;
            return left_cancelled(&running, fires, results, finish);
        }
        let checks = join_all(
            running
                .iter()
                .filter(|m| m.checked.is_none())
                .map(|m| async move { (m.index, m.member.check(settled).await) }),
        )
        .await;
        for (index, checked) in checks {
            if let Some(member) = running.iter_mut().find(|m| m.index == index) {
                member.checked = checked;
            }
        }
        let pending: Vec<_> = running.iter().filter_map(|m| m.member.pending()).collect();
        // one that fired takes the batch back, waiting on the others is for nothing
        let fired = pending.iter().any(|(pending, _)| *pending == Pending::Gone);
        let give_up = pending
            .iter()
            .filter_map(|(pending, at)| match pending {
                Pending::Armed(Some(fires_in)) => {
                    let at = *at + *fires_in;
                    Some(at.checked_sub(timing.margin).unwrap_or(at))
                }
                _ => None,
            })
            .chain([latest])
            .min()
            .unwrap_or(latest);
        if running.iter().all(|m| m.checked.is_some()) || fired || Instant::now() >= give_up {
            break;
        }
        let waiting: Vec<_> = running
            .iter()
            .filter(|m| m.checked.is_none())
            .map(|m| m.name.as_ref())
            .collect();
        info!("batch: waiting for {}", waiting.join(", "));
    }
    // reached, even with a part left, is reached (as after a single apply); else the last look
    // says why not
    for member in &mut running {
        match member.checked.take() {
            Some(checked) => {
                emit_checked(&member.log, &checked);
                member.checked = Some(checked);
            }
            None => {
                let remaining = member.member.last_look().await;
                emit_checked(&member.log, &remaining);
                member.checked = remaining.is_ok().then_some(remaining);
            }
        }
    }
    let unreached: Vec<Box<str>> = running
        .iter()
        .filter(|m| m.checked.is_none())
        .map(|m| m.name.clone())
        .collect();
    let in_line = |m: &Running<M>| matches!(&m.checked, Some(Ok(left)) if left.trim().is_empty());
    let not_applied: Vec<Box<str>> = running
        .iter()
        .filter(|m| m.checked.is_some() && !in_line(m))
        .map(|m| m.name.clone())
        .collect();
    if unreached.is_empty() && not_applied.is_empty() {
        let stopped = join_all(running.iter().map(|m| async {
            let stopped = match m.fallback {
                true => {
                    m.log.enter(ApplyState::StoppingFallback);
                    m.member.stop(fires, delay, &m.log).await
                }
                false => Ok(()),
            };
            match stopped {
                Ok(()) => {
                    m.log.enter(ApplyState::Finishing);
                    m.member.after(&m.log).await
                }
                Err(error) => Err(error),
            }
        }))
        .await;
        release_all(&running, true).await;
        for (member, stopped) in running.iter().zip(stopped) {
            results[member.index] = Some(stopped);
        }
    } else {
        let event = BatchIncomplete {
            devices: unreached.clone(),
            not_applied: not_applied.clone(),
        };
        let list: Box<str> = unreached
            .iter()
            .chain(&not_applied)
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>()
            .join(", ")
            .into();
        release_all(&running, false).await;
        for member in &running {
            member.log.enter(ApplyState::NotReached);
            member.log.emit(event.clone());
            let reason = if member.checked.is_none() {
                FallbackLeftReason::NotReached
            } else if !in_line(member) {
                FallbackLeftReason::NotApplied
            } else {
                FallbackLeftReason::BatchIncomplete
            };
            if member.fallback {
                fallback_left(&member.log, fires, reason, None);
            }
            results[member.index] = Some(Err(Error::BatchIncomplete(list.clone())));
        }
    }
    finish(results)
}

/// Before any script is started: takes the scripts and the fallbacks (of the `armed` ones)
/// away, on all at once.
async fn take_back<M: Applying>(running: &[Running<M>], armed: &[bool], fires: Instant, delay: Duration) {
    join_all(running.iter().zip(armed).map(|(m, &armed)| async move {
        // one cancelled before it armed has nothing on the device yet
        if !matches!(m.log.phase(), Some(JobPhase::Apply(ApplyState::Planning | ApplyState::SuspendingWatchdogs))) {
            m.log.enter(ApplyState::TakingBack);
        }
        if let Err(error) = m.member.remove().await {
            info!("{}: script not taken away: {error}", m.name);
        }
        if armed {
            let _ = m.member.stop(fires, delay, &m.log).await;
        }
    }))
    .await;
}

/// [`Applying::release`] on all, at once.
async fn release_all<M: Watched>(running: &[Running<M>], back: bool) {
    join_all(running.iter().map(|m| m.member.release(back, &m.log))).await;
}

/// Cancelled once the scripts may run: the fallbacks stay, as after a single apply's script.
fn left_cancelled<M>(
    running: &[Running<M>],
    fires: Instant,
    mut results: Vec<Option<Result<(), Error>>>,
    finish: impl FnOnce(Vec<Option<Result<(), Error>>>) -> Vec<Result<(), Error>>,
) -> Vec<Result<(), Error>> {
    for member in running {
        if member.fallback {
            fallback_left(&member.log, fires, FallbackLeftReason::Cancelled, None);
        }
        results[member.index] = Some(Err(Error::Cancelled));
    }
    finish(results)
}

/// [`Error::Cancelled`] once any job of the batch is cancelled.
fn cancelled(logs: &[Log]) -> Result<(), Error> {
    logs.iter().try_for_each(Log::check)
}

/// A cancel of one job is one of all of them.
fn cancel_all(logs: &[Log], error: &Error) {
    if matches!(error, Error::Cancelled) {
        logs.iter().for_each(Log::cancel);
    }
}

/// The devices at the other end of `device`'s wireless links.
pub fn wireless_partners(device: &DeviceAccess) -> Vec<DeviceAccess> {
    let mut found: HashMap<DeviceId, DeviceAccess> = HashMap::new();
    for port in device.interfaces() {
        if let Some(other) = port
            .wireless_peer()
            .and_then(|far| far.device())
            .filter(|other| other.id() != device.id())
        {
            found.insert(other.id(), other);
        }
    }
    found.into_values().collect()
}

#[cfg(test)]
mod test;
