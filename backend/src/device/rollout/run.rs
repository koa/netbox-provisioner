//! Carrying out a rollout (`JobKind::Rollout`): an ordinary update job per device of the plan,
//! the pilots first, then the rest in their order, up to [`PARALLEL`] at once, but never two
//! related ones (`PlannedDevice::related`: one behind the other, or powering it); the leases
//! keep a device from rebooting while one behind it updates (`dependency`). After the pilots and
//! after a device that failed it pauses until `continueRollout`; a failed device stays as it
//! is. Cancelled, it cancels the update jobs it started and starts none.
use super::{PlannedDevice, RolloutPlan};
use crate::{
    Error,
    error_info::ErrorInfo,
    job::{self, Job, JobState, Log},
    lock::LockAnyway,
};
use async_graphql::{Enum, SimpleObject};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    future::Future,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

/// How many devices update at once.
pub const PARALLEL: usize = 4;

/// How often it looks at the update jobs it started, and tries a device busy with another job
/// again.
const TICK: Duration = Duration::from_secs(2);

/// The rollout's plan, read again when it started.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutPlanned {
    pub plan: RolloutPlan,
}

/// A device of the rollout got to `state`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutDevice {
    pub device: u32,
    pub name: Box<str>,
    pub state: RolloutDeviceState,
    /// its update job, once started
    pub job: Option<u32>,
    /// why it failed
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RolloutDeviceState {
    /// it runs another job; tried again until that ends
    Waiting,
    Started,
    Done,
    Failed,
    /// its update job was cancelled
    Cancelled,
    /// left out on `skipRolloutDevice`
    Skipped,
}

/// The rollout waits for `continueRollout`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutPaused {
    pub reason: RolloutPause,
    /// the device that failed
    pub device: Option<Box<str>>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RolloutPause {
    /// the pilots are through: the rest only once they are seen to work
    AfterPilots,
    /// a device failed: the running ones ended, no other started
    Failed,
}

/// `continueRollout` let it go on.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutContinued {
    pub after: RolloutPause,
}

/// Every device of the plan had its turn.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutDone {
    pub updated: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// What the user tells a running rollout.
#[derive(Default)]
pub struct Control {
    state: Mutex<ControlState>,
    changed: Notify,
}

#[derive(Default)]
struct ControlState {
    go_on: bool,
    skip: BTreeSet<u32>,
}

/// The control of each rollout job, by its id.
static CONTROLS: LazyLock<Mutex<HashMap<u32, Arc<Control>>>> = LazyLock::new(Default::default);

impl Control {
    /// Makes `control` the one of rollout job `job`.
    pub fn register(job: u32, control: Arc<Control>) {
        let mut controls = CONTROLS.lock_anyway();
        controls.retain(|id, _| job::get(*id).is_some_and(|job| job.is_running()));
        // a rollout running already was given back: its control stays
        controls.entry(job).or_insert(control);
    }
    /// The control of the running rollout job `job`.
    pub fn of(job: u32) -> Option<Arc<Control>> {
        job::get(job).filter(|job| job.is_running())?;
        CONTROLS.lock_anyway().get(&job).cloned()
    }
    /// Lets a paused rollout go on.
    pub fn go_on(&self) {
        self.state.lock_anyway().go_on = true;
        self.changed.notify_waiters();
    }
    /// Leaves `device` out, unless its update started already.
    pub fn skip(&self, device: u32) {
        self.state.lock_anyway().skip.insert(device);
        self.changed.notify_waiters();
    }
    fn skipped(&self, device: u32) -> bool {
        self.state.lock_anyway().skip.contains(&device)
    }
    /// Waits for [`Self::go_on`]; one before the pause doesn't count.
    async fn paused(
        &self,
        log: &Log,
        reason: RolloutPause,
        device: Option<Box<str>>,
    ) -> Result<(), Error> {
        self.state.lock_anyway().go_on = false;
        log.emit(RolloutPaused { reason, device });
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if std::mem::take(&mut self.state.lock_anyway().go_on) {
                log.emit(RolloutContinued { after: reason });
                return Ok(());
            }
            log.unless_cancelled(notified).await?;
        }
    }
}

/// The update job of a device, as the rollout follows it.
pub trait UpdateJob {
    fn id(&self) -> u32;
    fn outcome(&self) -> (JobState, Option<ErrorInfo>);
    fn cancel(&self);
}

impl UpdateJob for Arc<Job> {
    fn id(&self) -> u32 {
        self.job_id()
    }
    fn outcome(&self) -> (JobState, Option<ErrorInfo>) {
        Job::outcome(self)
    }
    fn cancel(&self) {
        job::cancel(self.job_id());
    }
}

#[derive(Default)]
struct Tally {
    updated: u32,
    skipped: u32,
    failed: u32,
}

/// Carries out `plan`: `start` starts the update job of a device by its id.
pub async fn run<S, F, J>(
    plan: RolloutPlan,
    control: &Control,
    log: &Log,
    start: S,
) -> Result<(), Error>
where
    S: Fn(u32) -> F,
    F: Future<Output = Result<J, Error>>,
    J: UpdateJob,
{
    log.emit(RolloutPlanned { plan: plan.clone() });
    let (pilots, rest): (Vec<_>, Vec<_>) = plan
        .devices
        .iter()
        .filter(|device| device.skip.is_none())
        .partition(|device| device.pilot);
    let mut run = Run {
        control,
        log,
        start: &start,
        tally: Tally::default(),
        waiting: HashSet::new(),
    };
    let mut remaining = VecDeque::from(pilots);
    let mut after_pilots = !remaining.is_empty();
    let mut rest = Some(rest);
    loop {
        let (left, failed) = run.wave(remaining).await?;
        remaining = left;
        if let Some(failed) = failed {
            control
                .paused(log, RolloutPause::Failed, Some(failed))
                .await?;
            continue;
        }
        if let Some(rest) = rest.take() {
            if after_pilots && !rest.is_empty() {
                control.paused(log, RolloutPause::AfterPilots, None).await?;
            }
            after_pilots = false;
            remaining.extend(rest);
            continue;
        }
        if remaining.is_empty() {
            break;
        }
    }
    let Tally {
        updated,
        skipped,
        failed,
    } = run.tally;
    log.emit(RolloutDone {
        updated,
        skipped,
        failed,
    });
    Ok(())
}

struct Run<'a, S> {
    control: &'a Control,
    log: &'a Log,
    start: &'a S,
    tally: Tally,
    /// the devices seen busy with another job, told once
    waiting: HashSet<u32>,
}

impl<S, F, J> Run<'_, S>
where
    S: Fn(u32) -> F,
    F: Future<Output = Result<J, Error>>,
    J: UpdateJob,
{
    /// Updates `devices`, up to [`PARALLEL`] at once, until they are through or one failed:
    /// then none is started any more, the running ones end. Gives back those not started and
    /// the one that failed.
    async fn wave<'p>(
        &mut self,
        mut pending: VecDeque<&'p PlannedDevice>,
    ) -> Result<(VecDeque<&'p PlannedDevice>, Option<Box<str>>), Error> {
        let mut running: Vec<(&PlannedDevice, J)> = Vec::new();
        let mut failed = None;
        loop {
            let mut busy = Vec::new();
            while failed.is_none() && running.len() < PARALLEL {
                // the first in the order that none of the running ones is related to
                let free = pending
                    .iter()
                    .position(|device| !running.iter().any(|(other, _)| related(device, other)));
                let Some(device) = free.and_then(|at| pending.remove(at)) else {
                    break;
                };
                if self.control.skipped(device.id) {
                    self.tell(device, RolloutDeviceState::Skipped, None, None);
                    self.tally.skipped += 1;
                    continue;
                }
                match (self.start)(device.id).await {
                    Ok(job) => {
                        let id = Some(job.id());
                        self.tell(device, RolloutDeviceState::Started, id, None);
                        running.push((device, job));
                    }
                    Err(Error::Busy(_)) => {
                        if self.waiting.insert(device.id) {
                            self.tell(device, RolloutDeviceState::Waiting, None, None);
                        }
                        busy.push(device);
                    }
                    Err(error) => {
                        let error = Some(ErrorInfo::from(&error));
                        self.tell(device, RolloutDeviceState::Failed, None, error);
                        self.tally.failed += 1;
                        failed = Some(device.name.clone());
                    }
                }
            }
            // tried again at the next tick, before the others
            for device in busy.into_iter().rev() {
                pending.push_front(device);
            }
            if running.is_empty() && (pending.is_empty() || failed.is_some()) {
                return Ok((pending, failed));
            }
            if let Err(cancelled) = self.log.unless_cancelled(tokio::time::sleep(TICK)).await {
                for (_, job) in &running {
                    job.cancel();
                }
                return Err(cancelled);
            }
            let mut index = 0;
            while index < running.len() {
                let (device, job) = &running[index];
                let (state, error) = job.outcome();
                let told = match state {
                    JobState::Running => None,
                    JobState::Done => Some(RolloutDeviceState::Done),
                    JobState::Failed => Some(RolloutDeviceState::Failed),
                    JobState::Cancelled => Some(RolloutDeviceState::Cancelled),
                };
                let Some(told) = told else {
                    index += 1;
                    continue;
                };
                self.tell(device, told, Some(job.id()), error);
                if told == RolloutDeviceState::Done {
                    self.tally.updated += 1;
                } else {
                    self.tally.failed += 1;
                    failed.get_or_insert_with(|| device.name.clone());
                }
                running.swap_remove(index);
            }
        }
    }
    fn tell(
        &self,
        device: &PlannedDevice,
        state: RolloutDeviceState,
        job: Option<u32>,
        error: Option<ErrorInfo>,
    ) {
        self.log.emit(RolloutDevice {
            device: device.id,
            name: device.name.clone(),
            state,
            job,
            error,
        });
    }
}

/// `a` and `b` don't update at the same time: one is behind the other or powers it.
fn related(a: &PlannedDevice, b: &PlannedDevice) -> bool {
    a.related.contains(&b.id) || b.related.contains(&a.id)
}

#[cfg(test)]
mod test;
