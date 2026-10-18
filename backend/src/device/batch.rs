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
    error_info::ErrorInfo,
    device::{
        AccessibleDevice,
        radio_hold::RadioHold,
        ros::graphql::{
            APPLY_TIMEOUT, ApplyStarted, FALLBACK_MARGIN, FallbackLeftReason, PlannedApply,
            arm_fallback_on, emit_checked, fallback_left, remove_script, script_running,
            start_script, write_script,
        },
    },
    graphql::connect_for_job,
    job::Log,
    topology::{DeviceId, access::device::DeviceAccess},
};
use async_graphql::{SimpleObject, futures_util::future::join_all};
use log::info;
use mikrotik_api::{prelude::MikrotikDevice as RawDevice, simple::SimpleResult};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
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
/// time a radio takes to come back when one of them changes one.
fn fallback_delay() -> Duration {
    CONFIG.apply_fallback() + PREPARE_TIME + LEAD
}

/// The steps of one device's apply, as the batch runs them ([`run`]); a device reached over
/// the provisioner's connection ([`Linked`]) or, in the tests, a stand-in.
pub(crate) trait Applying: Send + Sync {
    /// Connects and plans the diff (`ApplyPlanned`); `false` when there is nothing to do.
    fn plan(&mut self, log: &Log) -> impl Future<Output = Result<bool, Error>> + Send;
    /// How much longer the fallback waits for a radio the plan changes, if it does.
    fn radio_wait(&self) -> Option<Duration>;
    /// Suspends the watchdogs a radio the plan changes may set off (`radio_hold`); nothing
    /// without one.
    fn hold(&mut self, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
    /// Restores what [`Self::hold`] suspended once the fallback won't fire (`back`), else leaves
    /// it to the scheduler entries.
    fn release(&self, back: bool, log: &Log) -> impl Future<Output = ()> + Send;
    /// Sets up the fallback, firing in `delay`, `radio_wait` of it for a radio
    /// (`FallbackArmed`); `false` where the device mode doesn't allow one (`FallbackUnavailable`).
    fn arm(
        &self,
        delay: Duration,
        radio_wait: Option<Duration>,
        log: &Log,
    ) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Leaves the script on the device, applying `lead` after it starts (`ApplyWritten`).
    fn write(&self, lead: Duration, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
    /// Starts the script; `true` once it runs.
    fn start(&self) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Whether the started script runs (by now).
    fn running(&self) -> impl Future<Output = Result<bool, Error>> + Send;
    /// Takes the script away before it is started (also one never written).
    fn remove(&self) -> impl Future<Output = Result<(), Error>> + Send;
    /// What is left once in line, or once `settled` passed; `None` while not, or not reached.
    fn check(&self, settled: Instant) -> impl Future<Output = Option<Result<Box<str>, Error>>> + Send;
    /// A last look at a device [`Self::check`] never found in line.
    fn last_look(&self) -> impl Future<Output = Result<Box<str>, Error>> + Send;
    /// Takes the fallback away (`FallbackStopped`, or `FallbackLeft`).
    fn stop(&self, fires: Instant, delay: Duration, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
    /// What follows the apply (a controller provisions its CAPs' radios again).
    fn after(&self, log: &Log) -> impl Future<Output = Result<(), Error>> + Send;
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
}

impl Linked {
    fn reached(&self) -> &Reached {
        self.reached.as_ref().expect("planned before")
    }
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
    async fn release(&self, back: bool, log: &Log) {
        let reached = self.reached();
        let hold = reached.hold.lock().unwrap_or_else(|p| p.into_inner()).take();
        match hold {
            Some(hold) if back => hold.release(&reached.link, log).await,
            Some(hold) => hold.leave(log),
            None => {}
        }
    }
    async fn arm(&self, delay: Duration, radio_wait: Option<Duration>, log: &Log) -> Result<bool, Error> {
        arm_fallback_on(&self.reached().control, delay, radio_wait, log).await
    }
    async fn write(&self, lead: Duration, log: &Log) -> Result<(), Error> {
        let reached = self.reached();
        write_script(&reached.control, &reached.planned, lead, log).await
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
    async fn check(&self, settled: Instant) -> Option<Result<Box<str>, Error>> {
        self.reached().link.check_applied(settled).await
    }
    async fn last_look(&self) -> Result<Box<str>, Error> {
        self.reached().link.remaining_diff().await
    }
    async fn stop(&self, fires: Instant, delay: Duration, log: &Log) -> Result<(), Error> {
        self.reached().link.stop_fallback(fires, delay, log).await
    }
    async fn after(&self, log: &Log) -> Result<(), Error> {
        let reached = self.reached();
        reached.link.after_apply(&reached.planned, log).await
    }
}

/// The times of a batch: the fallback's delay, how long a script waits once started and how
/// long starting it may take, how often and how long it looks for the devices, and how long
/// before the fallback fires it stops.
pub(crate) struct Timing {
    pub delay: Duration,
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
    let batch = Arc::new(devices.iter().map(DeviceAccess::id).collect::<BTreeSet<_>>());
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
    run(
        members,
        names,
        logs,
        Timing {
            delay: fallback_delay(),
            lead: LEAD,
            start: START_TIME,
            poll: Duration::from_secs(3),
            settle: APPLY_TIMEOUT,
            margin: FALLBACK_MARGIN,
        },
    )
    .await
}

/// One member while the batch runs.
struct Running<M> {
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
            Ok(()) => m.member.hold(&m.log).await,
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
    let fires = Instant::now() + delay;
    let prepared = join_all(running.iter().map(|m| async {
        let armed = async {
            cancelled(&logs)?;
            m.member.arm(fires.saturating_duration_since(Instant::now()), radio_wait, &m.log).await
        }
        .await;
        match armed {
            Ok(fallback) => (Some(fallback), m.member.write(timing.lead, &m.log).await),
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
    join_all(running.iter().map(|m| async move {
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
    }))
    .await;
    // from here on the scripts apply without the provisioner: cancelled, the fallbacks stay
    sleep(timing.lead.saturating_sub(start.elapsed())).await;

    // until every device is reached and checked, or the fallbacks are about to fire
    let settled = Instant::now() + timing.settle;
    let give_up = fires.checked_sub(timing.margin).unwrap_or(fires);
    loop {
        sleep(timing.poll).await;
        if cancelled(&logs).is_err() {
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
        if running.iter().all(|m| m.checked.is_some()) || Instant::now() >= give_up {
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
                true => m.member.stop(fires, delay, &m.log).await,
                false => Ok(()),
            };
            match stopped {
                Ok(()) => m.member.after(&m.log).await,
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
async fn release_all<M: Applying>(running: &[Running<M>], back: bool) {
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
mod run_test {
    //! The rules of a batch, with stand-ins for the devices.
    use super::{Applying, Timing, run};
    use crate::{
        Error,
        device::ros::graphql::FallbackLeftReason,
        job::{Event, Log},
    };
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    #[derive(Clone, Default)]
    struct Fake {
        name: &'static str,
        nothing_to_do: bool,
        plan_fails: bool,
        arm_fails: bool,
        write_fails: bool,
        /// its script doesn't start
        start_fails: bool,
        /// not reached after its script
        lost: bool,
        /// reached after its script, but not in line
        left: bool,
        /// its plan changes a radio
        radio: bool,
        /// its device mode has the scheduler off: no fallback
        no_scheduler: bool,
        calls: Arc<Mutex<Vec<String>>>,
        delays: Arc<Mutex<Vec<Duration>>>,
    }

    impl Fake {
        fn call(&self, what: &str) {
            self.calls.lock().unwrap().push(format!("{what} {}", self.name));
        }
        fn fails(&self, fails: bool) -> Result<(), Error> {
            if fails { Err(Error::MissingCredentials) } else { Ok(()) }
        }
    }

    impl Applying for Fake {
        async fn plan(&mut self, _log: &Log) -> Result<bool, Error> {
            self.call("plan");
            self.fails(self.plan_fails)?;
            Ok(!self.nothing_to_do)
        }
        fn radio_wait(&self) -> Option<Duration> {
            self.radio.then_some(Duration::from_millis(200))
        }
        async fn hold(&mut self, _log: &Log) -> Result<(), Error> {
            if self.radio {
                self.call("hold");
            }
            Ok(())
        }
        async fn release(&self, back: bool, _log: &Log) {
            if self.radio {
                self.call(if back { "release" } else { "keep" });
            }
        }
        async fn arm(&self, delay: Duration, _radio_wait: Option<Duration>, _log: &Log) -> Result<bool, Error> {
            self.delays.lock().unwrap().push(delay);
            self.call("arm");
            self.fails(self.arm_fails)?;
            Ok(!self.no_scheduler)
        }
        async fn write(&self, _lead: Duration, _log: &Log) -> Result<(), Error> {
            self.call("write");
            self.fails(self.write_fails)
        }
        async fn start(&self) -> Result<bool, Error> {
            self.call("start");
            self.fails(self.start_fails)?;
            Ok(true)
        }
        async fn running(&self) -> Result<bool, Error> {
            Ok(true)
        }
        async fn remove(&self) -> Result<(), Error> {
            self.call("remove");
            Ok(())
        }
        async fn check(&self, _settled: Instant) -> Option<Result<Box<str>, Error>> {
            // a script not started leaves its device as it was
            (!self.lost).then(|| Ok(if self.left || self.start_fails { "set x".into() } else { "".into() }))
        }
        async fn last_look(&self) -> Result<Box<str>, Error> {
            if self.lost { Err(Error::MissingCredentials) } else { Ok("".into()) }
        }
        async fn stop(&self, _fires: Instant, _delay: Duration, _log: &Log) -> Result<(), Error> {
            self.call("stop");
            Ok(())
        }
        async fn after(&self, _log: &Log) -> Result<(), Error> {
            Ok(())
        }
    }

    /// `near` and `far`: the calls and the outcomes.
    async fn batch(near: Fake, far: Fake) -> (Vec<String>, Vec<Result<(), Error>>, Vec<Log>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let delays = Arc::new(Mutex::new(Vec::new()));
        let near = Fake { name: "near", calls: calls.clone(), delays: delays.clone(), ..near };
        let far = Fake { name: "far", calls: calls.clone(), delays: delays.clone(), ..far };
        let logs = vec![Log::new("near"), Log::new("far")];
        let results = run(
            vec![near, far],
            vec!["near".into(), "far".into()],
            logs.clone(),
            Timing {
                delay: Duration::from_millis(300),
                lead: Duration::from_millis(5),
                start: Duration::from_millis(100),
                poll: Duration::from_millis(10),
                settle: Duration::ZERO,
                margin: Duration::ZERO,
            },
        )
        .await;
        let calls = calls.lock().unwrap().clone();
        (calls, results, logs)
    }

    fn left(log: &Log) -> Vec<FallbackLeftReason> {
        log.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::FallbackLeft(left) => Some(left.reason),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn all_back_takes_every_fallback_away() {
        let (calls, results, _) = batch(Fake::default(), Fake::default()).await;
        assert_eq!(
            calls,
            [
                "plan near", "plan far", "arm near", "write near", "arm far", "write far",
                "start near", "start far", "stop near", "stop far"
            ]
        );
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }

    /// A device whose device mode has the scheduler off has no fallback to take away or leave.
    #[tokio::test]
    async fn a_device_without_scheduler_has_no_fallback_to_stop() {
        let (calls, results, _) = batch(Fake { no_scheduler: true, ..Fake::default() }, Fake::default()).await;
        assert!(calls.contains(&"stop far".to_string()), "{calls:?}");
        assert!(!calls.contains(&"stop near".to_string()), "{calls:?}");
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        let (_, _, logs) = batch(
            Fake { no_scheduler: true, ..Fake::default() },
            Fake { lost: true, ..Fake::default() },
        )
        .await;
        assert_eq!(left(&logs[0]), []);
        assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
    }

    #[tokio::test]
    async fn one_not_back_leaves_every_fallback() {
        let (calls, results, logs) = batch(Fake::default(), Fake { lost: true, ..Fake::default() }).await;
        assert!(!calls.iter().any(|call| call.starts_with("stop")), "{calls:?}");
        assert!(
            results.iter().all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "far")),
            "{results:?}"
        );
        assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
        assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
    }

    /// lab-w60-b: reached over another way, but its script never ran
    #[tokio::test]
    async fn one_back_but_not_in_line_leaves_every_fallback() {
        let (calls, results, logs) = batch(Fake::default(), Fake { left: true, ..Fake::default() }).await;
        assert!(!calls.iter().any(|call| call.starts_with("stop")), "{calls:?}");
        assert!(results.iter().all(|r| matches!(r, Err(Error::BatchIncomplete(_)))), "{results:?}");
        assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
        assert_eq!(left(&logs[1]), [FallbackLeftReason::NotApplied]);
    }

    /// the others' scripts go on; the one not started leaves its device not in line
    #[tokio::test]
    async fn a_script_not_started_sends_all_back() {
        let (calls, results, logs) = batch(Fake { start_fails: true, ..Fake::default() }, Fake::default()).await;
        assert!(calls.contains(&"start far".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|call| call.starts_with("stop") || call.starts_with("remove")), "{calls:?}");
        assert!(results.iter().all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "near")), "{results:?}");
        assert_eq!(left(&logs[0]), [FallbackLeftReason::NotApplied]);
        assert_eq!(left(&logs[1]), [FallbackLeftReason::BatchIncomplete]);
        let started_with_error = logs[0].events().into_iter().any(|event| {
            matches!(event, Event::ApplyStarted(started) if started.error.is_some())
        });
        assert!(started_with_error);
    }

    /// a radio changed on one end keeps the other end off as long: both wait for it
    #[tokio::test]
    async fn a_radio_change_lengthens_every_fallback() {
        let delays = Arc::new(Mutex::new(Vec::new()));
        let near = Fake { name: "near", radio: true, delays: delays.clone(), ..Fake::default() };
        let far = Fake { name: "far", delays: delays.clone(), ..Fake::default() };
        let results = run(
            vec![near, far],
            vec!["near".into(), "far".into()],
            vec![Log::new("near"), Log::new("far")],
            Timing {
                delay: Duration::from_millis(300),
                lead: Duration::from_millis(5),
                start: Duration::from_millis(100),
                poll: Duration::from_millis(10),
                settle: Duration::ZERO,
                margin: Duration::ZERO,
            },
        )
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        let delays = delays.lock().unwrap();
        assert_eq!(delays.len(), 2);
        assert!(delays.iter().all(|d| *d > Duration::from_millis(400)), "{delays:?}");
    }

    /// A changed radio holds the watchdogs before the fallbacks, restores them once all are
    /// back or nothing is applied, and leaves them while the fallbacks may still fire.
    #[tokio::test]
    async fn a_changed_radio_holds_the_watchdogs_until_all_are_back() {
        let radio = || Fake { radio: true, ..Fake::default() };
        let (calls, results, _) = batch(radio(), Fake::default()).await;
        assert_eq!(
            calls,
            [
                "plan near", "plan far", "hold near", "arm near", "write near", "arm far",
                "write far", "start near", "start far", "stop near", "stop far", "release near"
            ]
        );
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        let (calls, _, _) = batch(radio(), Fake { lost: true, ..Fake::default() }).await;
        assert_eq!(calls.last().map(String::as_str), Some("keep near"), "{calls:?}");
        let (calls, _, _) = batch(radio(), Fake { write_fails: true, ..Fake::default() }).await;
        assert_eq!(calls.last().map(String::as_str), Some("release near"), "{calls:?}");
    }

    #[tokio::test]
    async fn a_failing_write_starts_nothing() {
        let (calls, results, _) = batch(Fake { write_fails: true, ..Fake::default() }, Fake::default()).await;
        assert_eq!(
            calls,
            [
                "plan near", "plan far", "arm near", "write near", "arm far", "write far",
                "remove near", "stop near", "remove far", "stop far"
            ]
        );
        assert!(matches!(&results[0], Err(Error::MissingCredentials)), "{results:?}");
        assert!(matches!(&results[1], Err(Error::BatchStopped(d)) if &**d == "near"), "{results:?}");
    }

    #[tokio::test]
    async fn a_failing_plan_applies_nothing() {
        let (calls, results, _) = batch(Fake { plan_fails: true, ..Fake::default() }, Fake::default()).await;
        assert_eq!(calls, ["plan near", "plan far"]);
        assert!(matches!(&results[0], Err(Error::MissingCredentials)), "{results:?}");
        assert!(matches!(&results[1], Err(Error::BatchStopped(d)) if &**d == "near"), "{results:?}");
    }

    #[tokio::test]
    async fn each_failing_plan_keeps_its_own_error() {
        let failing = || Fake { plan_fails: true, ..Fake::default() };
        let (_, results, _) = batch(failing(), failing()).await;
        assert!(results.iter().all(|r| matches!(r, Err(Error::MissingCredentials))), "{results:?}");
    }

    #[tokio::test]
    async fn a_failing_fallback_takes_the_others_away() {
        let (calls, results, _) = batch(Fake { arm_fails: true, ..Fake::default() }, Fake::default()).await;
        assert_eq!(
            calls,
            ["plan near", "plan far", "arm near", "arm far", "write far", "remove near", "remove far", "stop far"]
        );
        assert!(matches!(&results[1], Err(Error::BatchStopped(_))), "{results:?}");
    }

    #[tokio::test]
    async fn a_device_in_line_is_left_out() {
        let (calls, results, _) = batch(Fake { nothing_to_do: true, ..Fake::default() }, Fake::default()).await;
        assert_eq!(calls, ["plan near", "plan far", "arm far", "write far", "start far", "stop far"]);
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
}

#[cfg(test)]
mod test {
    use super::wireless_partners;
    use crate::topology::{
        Cable, CablePort, Device, DeviceId, Interface, InterfaceId, PortType, RadioLink,
        WirelessLinkEnd, test::TopologyBuilder,
    };
    use std::sync::Arc;

    fn device(builder: &mut TopologyBuilder, name: &str) -> DeviceId {
        let id = builder.devices.next_id();
        builder.devices.insert(
            id,
            Device {
                name: name.into(),
                has_routeros: true,
                ..Device::default()
            },
        );
        id
    }
    fn port(builder: &mut TopologyBuilder, device: DeviceId, name: &str, link: Option<WirelessLinkEnd>) -> InterfaceId {
        let id = builder.interfaces.next_id();
        builder.interfaces.insert(
            id,
            Interface {
                name: name.into(),
                device,
                port_type: Some(if link.is_some() { PortType::Wireless } else { PortType::Ethernet }),
                wireless_link: link,
                ..Interface::default()
            },
        );
        id
    }
    fn cable(builder: &mut TopologyBuilder, a: InterfaceId, b: InterfaceId) {
        let id = builder.cables.next_id();
        builder.cables.insert(
            id,
            Cable {
                port_a: Box::new([CablePort::Interface(a)]),
                port_b: Box::new([CablePort::Interface(b)]),
            },
        );
    }
    fn link_end(peer: InterfaceId) -> WirelessLinkEnd {
        WirelessLinkEnd {
            id: 1,
            peer: Some(peer),
            link: RadioLink {
                ssid: "link-5".into(),
                encrypted: false,
                psk: None,
                country: None,
                frequency_mode: None,
            },
        }
    }

    /// core -cable- burri -cable- ap ~radio~ station -cable- stall
    #[test]
    fn finds_the_other_end_of_a_radio_link() {
        let mut b = TopologyBuilder::default();
        let core = device(&mut b, "core");
        let burri = device(&mut b, "burri");
        let ap = device(&mut b, "link-5-ap");
        let station = device(&mut b, "link-5-station");
        let stall = device(&mut b, "stall");
        let c1 = port(&mut b, core, "ether1", None);
        let b1 = port(&mut b, burri, "ether1", None);
        let b2 = port(&mut b, burri, "ether2", None);
        let a1 = port(&mut b, ap, "ether1", None);
        let s1 = port(&mut b, station, "ether1", None);
        let t1 = port(&mut b, stall, "ether5", None);
        cable(&mut b, c1, b1);
        cable(&mut b, b2, a1);
        cable(&mut b, s1, t1);
        // the radios point at each other; their ids are the next two
        let next = b.interfaces.next_id();
        let (ap_radio, station_radio) = (next, InterfaceId(next.0 + 1));
        b.interfaces.insert(
            ap_radio,
            Interface {
                name: "wlan1".into(),
                device: ap,
                port_type: Some(PortType::Wireless),
                wireless_link: Some(link_end(station_radio)),
                ..Interface::default()
            },
        );
        let _ = b.interfaces.next_id();
        b.interfaces.insert(
            station_radio,
            Interface {
                name: "wlan1".into(),
                device: station,
                port_type: Some(PortType::Wireless),
                wireless_link: Some(link_end(ap_radio)),
                ..Interface::default()
            },
        );
        let topology = Arc::new(b.build());
        let get = |id| topology.get_device_by_id(&id).expect("device");
        let partners: Vec<_> = wireless_partners(&get(ap)).iter().map(|d| d.name().to_string()).collect();
        assert_eq!(partners, ["link-5-station"]);
        assert!(wireless_partners(&get(burri)).is_empty());
    }
}
