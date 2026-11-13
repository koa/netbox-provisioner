//! Scope: Running work in the background as a job: starting, its log and events, cancelling, the
//! list of jobs and the shutdown; not the work itself (`device::…`) nor its state machines
//! (`device_state`).
//!
//! Long running work on a device (an update, an apply, the initial setup) runs as a job in the
//! background: the request that starts it returns at once, the UI asks for the job's events as
//! they come and can cancel it. Events carry what happened, not text: the UI words them (in
//! German) and can offer actions on them.
//!
//! A device runs one job at a time. Cancelling is up to the work: it stops where that is safe,
//! between its steps ([`Log::check`]) and while it waits ([`Log::unless_cancelled`]), not
//! inside a step that would leave the device half changed (the upload with `/ip ssh` switched
//! for it). What it started on the device keeps going there (an uploaded package installs at
//! the next boot). Jobs live in the provisioner process only, the last [`KEEP`] finished ones
//! are kept. Their numbers start anew after each start of the provisioner ([`first_id`]), so a
//! page still asking for a job of the run before gets no job rather than someone else's.
use crate::{
    Error,
    device::{
        self, batch, bootstrap, dependency, device_mode, leftovers, partitions, power_cycle,
        radio_hold, reboot,
        ros::{apply, cfg::GeneratedCfg},
        terminal, update, update_fallback, watchdog,
    },
    device_state::{JobPhase, Restart},
    error_info::ErrorInfo,
    facts::{self, FactKind},
    graphql::scalars::ScalarDuration,
    helper,
    journal::{self, Started},
    lock::LockAnyway,
    saved_job::{self, SavedJob},
    topology::{
        DeviceId, SiteId,
        access::{AccessTopology, device::DeviceAccess, site::SiteAccess},
    },
};
use async_graphql::{Enum, Object, SimpleObject, Union};
use log::{info, warn};
use std::{
    sync::{
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Notify, watch};

pub mod resume;

/// How many finished jobs are kept.
const KEEP: usize = 50;

/// Something a job did or waits for.
#[derive(Debug, Clone, PartialEq, Union)]
pub enum Event {
    Connecting(device::Connecting),
    Connected(device::Connected),
    PoeChain(dependency::PoeChain),
    WaitingForDevice(dependency::WaitingForDevice),
    UpdateTarget(update::UpdateTarget),
    UpdateStart(update::UpdateStart),
    UpdateStepPlanned(update::UpdateStepPlanned),
    SshPasswordLogin(update::SshPasswordLogin),
    PackageDownloaded(update::PackageDownloaded),
    PackageUploading(update::PackageUploading),
    PackageUploaded(update::PackageUploaded),
    Reboot(reboot::Reboot),
    PowerCycled(power_cycle::PowerCycled),
    RouterOsRunning(update::RouterOsRunning),
    FirmwareUpgrade(update::FirmwareUpgrade),
    UpdateDone(update::UpdateDone),
    Downgraded(update::Downgraded),
    LeftoverCleared(leftovers::LeftoverCleared),
    ApplyPlanned(apply::ApplyPlanned),
    FallbackArmed(apply::FallbackArmed),
    FallbackUnavailable(apply::FallbackUnavailable),
    ApplyRunning(apply::ApplyRunning),
    ApplyWritten(apply::ApplyWritten),
    ApplyStarted(apply::ApplyStarted),
    ApplyChecked(apply::ApplyChecked),
    FallbackStopped(apply::FallbackStopped),
    FallbackLeft(apply::FallbackLeft),
    FallbackFired(apply::FallbackFired),
    ScriptProgress(apply::ScriptProgress),
    RadiosReprovisioned(apply::RadiosReprovisioned),
    PasswordSet(apply::PasswordSet),
    WwwCertificateInstalled(crate::device::www_certificate::WwwCertificateInstalled),
    BatchApply(batch::BatchApply),
    BatchStopped(batch::BatchStopped),
    BatchIncomplete(batch::BatchIncomplete),
    SetupStarted(bootstrap::SetupStarted),
    ConsoleOutput(terminal::ConsoleOutput),
    WaitingForApi(bootstrap::WaitingForApi),
    SetupDone(bootstrap::SetupDone),
    DeviceModeConfirm(device_mode::DeviceModeConfirm),
    DeviceModeChanged(device_mode::DeviceModeChanged),
    RepartitionStarted(partitions::RepartitionStarted),
    Repartitioned(partitions::Repartitioned),
    PartitionRunning(partitions::PartitionRunning),
    PartitionSynced(partitions::PartitionSynced),
    UpdateFallbackArmed(update_fallback::UpdateFallbackArmed),
    UpdateFallbackDisarmed(update_fallback::UpdateFallbackDisarmed),
    UpdateWithoutFallback(update_fallback::UpdateWithoutFallback),
    OspfNeighborsBack(update_fallback::OspfNeighborsBack),
    WatchdogNotSet(watchdog::WatchdogNotSet),
    WatchdogSuspended(watchdog::WatchdogSuspended),
    WatchdogRestored(watchdog::WatchdogRestored),
    WatchdogNotRestored(watchdog::WatchdogNotRestored),
    WatchdogLeft(watchdog::WatchdogLeft),
    WatchdogStillHeld(watchdog::WatchdogStillHeld),
    WatchdogKept(radio_hold::WatchdogKept),
    JournalNotWritten(journal::JournalNotWritten),
    Resumed(Resumed),
    NetboxWritten(helper::NetboxWritten),
    NetboxUndone(helper::NetboxUndone),
    SiteDiffStarted(device::site_diff::SiteDiffStarted),
    DeviceDiffed(device::site_diff::DeviceDiffed),
    RolloutPlanned(device::rollout::run::RolloutPlanned),
    RolloutDevice(device::rollout::run::RolloutDevice),
    RolloutPaused(device::rollout::run::RolloutPaused),
    RolloutContinued(device::rollout::run::RolloutContinued),
    RolloutDone(device::rollout::run::RolloutDone),
}

/// The job goes on after a restart of the provisioner, in the state it was saved in
/// (`saved_job`), `after` it was saved.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Resumed {
    pub after: ScalarDuration,
}

/// An event and when it happened, counted from the start of the job.
#[derive(Debug, Clone, SimpleObject)]
pub struct Logged {
    /// its position in the job's events, for asking only for the ones after
    pub index: u32,
    pub at: ScalarDuration,
    pub event: Event,
}

/// Where a job's events go and how its work learns that it is cancelled; also usable without
/// a job (tests, the CHR harness).
#[derive(Clone)]
pub struct Log {
    label: Box<str>,
    started: Instant,
    events: Arc<Mutex<Vec<Logged>>>,
    cancelled: Arc<(AtomicBool, Notify)>,
    phase: Arc<Mutex<Phases>>,
    saving: Arc<Mutex<Saving>>,
    /// ticks the job's journal entry to be written again (`crate::journal`), unset without one
    journal: Arc<OnceLock<watch::Sender<()>>>,
    /// that entry's id in NetBox once created, saved with the job (`SavedJob::journal_entry`)
    journal_entry: Arc<OnceLock<u64>>,
    /// the devices the job works on and through, whose pages hear of its events
    /// (`facts::Change::Job`); unset without a job
    devices: Arc<OnceLock<Box<[DeviceId]>>>,
    /// the run of the apply script it wrote, as its marks on the device name it; in its journal
    /// entry, so a fallback seen fired later finds them (`journal::observe`)
    script_run: Arc<Mutex<Option<Box<str>>>>,
}

/// What the job keeps for going on after a restart ([`Log::keep`]), and whether its file is
/// written now.
#[derive(Debug, Default)]
struct Saving {
    job: Option<SavedJob>,
    written: bool,
}

/// The state a job is in (`device_state`), and the transitions its tables don't allow.
#[derive(Debug, Default)]
struct Phases {
    current: Option<JobPhase>,
    /// the last state of the job's own work: `current`, unless that undoes for others
    /// ([`JobPhase::undoes_for_others`], restoring watchdogs after a failure)
    own: Option<JobPhase>,
    /// `current` is the state a job going on after a restart was saved in: entering it again
    /// once is no transition ([`Log::resume_in`])
    resumed: bool,
    unexpected: Vec<Transition>,
}

impl Log {
    /// `label` names the device in the server's own log.
    pub fn new(label: &str) -> Log {
        Log {
            label: label.into(),
            started: Instant::now(),
            events: Default::default(),
            cancelled: Default::default(),
            phase: Default::default(),
            saving: Default::default(),
            journal: Default::default(),
            journal_entry: Default::default(),
            devices: Default::default(),
            script_run: Default::default(),
        }
    }
    /// The job wrote the apply script of `run` onto the device.
    pub fn wrote_script(&self, run: &str) {
        *self.script_run.lock_anyway() = Some(run.into());
    }
    /// The run of the apply script it wrote last.
    pub fn script_run(&self) -> Option<Box<str>> {
        self.script_run.lock_anyway().clone()
    }
    /// The job enters `phase`. One its kind's table doesn't allow after the current one is
    /// logged and kept ([`Self::unexpected`]), the job goes on as before.
    pub fn enter(&self, phase: impl Into<JobPhase>) {
        let phase = phase.into();
        let mut phases = self.phase.lock_anyway();
        let again = phases.resumed && phases.current == Some(phase);
        phases.resumed = false;
        if !again && !phase.may_follow(phases.current) {
            warn!(
                "{}: unexpected state {phase:?} after {:?}",
                self.label, phases.current
            );
            let before = phases.current;
            phases.unexpected.push(Transition {
                from: before,
                to: phase,
            });
        }
        phases.current = Some(phase);
        if !phase.undoes_for_others() {
            phases.own = Some(phase);
        }
        drop(phases);
        self.save_for(phase, true);
        self.changed();
    }
    /// A job going on after a restart starts in the state it was saved in, whatever its kind's
    /// first states are; its work may enter that state again.
    pub fn resume_in(&self, phase: impl Into<JobPhase>) {
        let phase = phase.into();
        let mut phases = self.phase.lock_anyway();
        phases.current = Some(phase);
        if !phase.undoes_for_others() {
            phases.own = Some(phase);
        }
        phases.resumed = true;
        drop(phases);
        self.save_for(phase, false);
    }
    /// The job's journal entry is `id` in NetBox.
    pub fn journal_entry_is(&self, id: u64) {
        let _ = self.journal_entry.set(id);
    }
    /// Has the job's journal entry written again.
    fn changed(&self) {
        if let Some(journal) = self.journal.get() {
            journal.send_replace(());
        }
        self.announce();
    }
    /// The devices the job works on and through; none for a site's job.
    pub fn devices(&self) -> &[DeviceId] {
        self.devices.get().map_or(&[], |devices| devices)
    }
    /// Tells the subscriptions of the job's devices that it changed.
    fn announce(&self) {
        for device in self.devices.get().into_iter().flatten() {
            facts::announce(facts::Change::Job(*device));
        }
    }
    /// Keeps what the job needs to go on after a restart (`saved_job`), from the next state it
    /// enters on: written whenever it enters a state that goes on (`Restart::Continue`), with
    /// that state, and removed when it enters any other or ends.
    pub fn keep(&self, job: SavedJob) {
        self.saving.lock_anyway().job = Some(job);
    }
    /// Writes or removes the file for `phase`; `entered` now, else entered before a restart: the
    /// update counts its reboot from then (`saved_at`).
    fn save_for(&self, phase: JobPhase, entered: bool) {
        let mut saving = self.saving.lock_anyway();
        let Saving { job, written } = &mut *saving;
        let Some(job) = job else {
            return;
        };
        if phase.restart() == Restart::Continue && job.work.set_phase(phase) {
            if entered {
                job.saved_at = SystemTime::now();
            }
            job.journal_entry = self.journal_entry.get().copied().or(job.journal_entry);
            saved_job::save(job);
            *written = true;
        } else if *written {
            saved_job::remove(job.device);
            *written = false;
        }
    }
    /// What the file holds now, `None` while there is none.
    #[cfg(test)]
    fn saved(&self) -> Option<SavedJob> {
        let saving = self.saving.lock_anyway();
        saving.job.clone().filter(|_| saving.written)
    }
    /// The job ended: what it kept for a restart goes, but while the server shuts down
    /// ([`shutdown`]) in a state that goes on.
    fn ended(&self) {
        if SHUTTING_DOWN.load(Ordering::SeqCst) && self.goes_on() {
            return;
        }
        let mut saving = self.saving.lock_anyway();
        if let Some(job) = saving.job.take()
            && saving.written
        {
            saved_job::remove(job.device);
        }
        saving.written = false;
    }
    /// Its file is written: it goes on after a restart.
    fn goes_on(&self) -> bool {
        self.saving.lock_anyway().written
    }
    /// The state the job is in, or ended in; `None` before its first.
    pub fn phase(&self) -> Option<JobPhase> {
        self.phase.lock_anyway().current
    }
    /// The state the job's own work ended in: the last one before undoing for others (where it
    /// failed, not where it restored the watchdogs afterwards); the current one without such.
    pub fn ended_in(&self) -> Option<JobPhase> {
        let phases = self.phase.lock_anyway();
        phases.own.or(phases.current)
    }
    /// The transitions the tables didn't allow.
    pub fn unexpected(&self) -> Vec<Transition> {
        self.phase.lock_anyway().unexpected.clone()
    }
    /// Asks the work to stop ([`cancel`] for a job).
    pub fn cancel(&self) {
        let (cancelled, notify) = &*self.cancelled;
        cancelled.store(true, Ordering::SeqCst);
        notify.notify_waiters();
    }
    /// [`Error::Cancelled`] once the job is cancelled; called between steps.
    pub fn check(&self) -> Result<(), Error> {
        if self.cancelled.0.load(Ordering::SeqCst) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    /// Runs `work` (a wait, or a step that may stop anywhere) unless the job is cancelled
    /// before it ends.
    pub async fn unless_cancelled<T>(&self, work: impl Future<Output = T>) -> Result<T, Error> {
        let (_, notify) = &*self.cancelled;
        // registered before the check, so a cancel in between still wakes us
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        self.check()?;
        tokio::select! {
            result = work => Ok(result),
            () = notified => Err(Error::Cancelled),
        }
    }
    pub fn emit(&self, event: impl Into<Event>) {
        let event = event.into();
        info!("{}: {event:?}", self.label);
        let mut events = self.events.lock_anyway();
        let index = events.len() as u32;
        events.push(Logged {
            index,
            at: self.started.elapsed().into(),
            event,
        });
        drop(events);
        self.announce();
    }
    /// The events from `after` on.
    pub fn since(&self, after: u32) -> Vec<Logged> {
        let events = self.events.lock_anyway();
        events.get(after as usize..).unwrap_or_default().to_vec()
    }
    pub fn events(&self) -> Vec<Event> {
        self.since(0)
            .into_iter()
            .map(|logged| logged.event)
            .collect()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum JobKind {
    /// RouterOS and firmware update (`updateRouteros`)
    Update,
    /// RouterOS taken back to an older version (`downgradeRouteros`)
    Downgrade,
    /// the generated config (`applyCfg`)
    Apply,
    /// something a job left on the device taken care of (`clearLeftover`)
    Cleanup,
    /// initial setup over mac-telnet (`bootstrap`)
    Bootstrap,
    /// a reboot asked for on the device page (`rebootDevice`)
    Reboot,
    /// PoE switched off and on for the device it feeds (`powerCycle`), a job of the supplier
    PowerCycle,
    /// features turned on in the device mode (`changeDeviceMode`)
    DeviceMode,
    /// the flash split in partitions (`repartitionDevice`)
    Repartition,
    /// booting from another partition (`bootPartition`)
    BootPartition,
    /// a helper's plan written to NetBox (`writeNetboxPlan`)
    NetboxWrite,
    /// what a `NetboxWrite` job wrote taken back (`undoNetboxWrite`)
    NetboxUndo,
    /// how far each device of a site is from NetBox (`diffSite`), read only
    SiteDiff,
    /// an update job per device of several sites (`startRollout`), a job of the network
    Rollout,
}

impl JobKind {
    /// The facts about its device a job of the kind may change, dropped when it ends.
    /// What the device sees (`Neighbors`, `InterfaceLive`) goes with every job touching it: an
    /// apply renames its ports and sets the discovery list, a reboot loses the neighbors for a
    /// while.
    pub fn invalidates(self) -> &'static [FactKind] {
        use FactKind::*;
        match self {
            // `Partitions`: the other one follows the apply (`PartitionSynced`)
            JobKind::Apply => &[
                Login,
                Route,
                Config,
                Leftovers,
                Partitions,
                Interfaces,
                Neighbors,
                InterfaceLive,
            ],
            JobKind::Update | JobKind::Downgrade => &[
                Login,
                Route,
                Config,
                RouterosState,
                Leftovers,
                Partitions,
                DeviceMode,
                Interfaces,
                Neighbors,
                InterfaceLive,
            ],
            JobKind::Cleanup => &[
                Login,
                Route,
                Config,
                RouterosState,
                Leftovers,
                Neighbors,
                InterfaceLive,
            ],
            JobKind::Bootstrap => &[
                Login,
                Route,
                Config,
                RouterosState,
                Leftovers,
                Interfaces,
                Neighbors,
                InterfaceLive,
            ],
            JobKind::Reboot => &[Login, Route, Config, Leftovers, Neighbors, InterfaceLive],
            // a job of the supplier: what it sees on the port changes with the device it feeds
            JobKind::PowerCycle => &[Login, Route, Leftovers, Neighbors, InterfaceLive],
            // `Partitions`: whether they can be split depends on the device mode
            JobKind::DeviceMode => &[
                Login,
                Route,
                Config,
                Leftovers,
                Partitions,
                DeviceMode,
                Neighbors,
                InterfaceLive,
            ],
            JobKind::Repartition | JobKind::BootPartition => &[
                Login,
                Route,
                Config,
                RouterosState,
                Leftovers,
                Partitions,
                Neighbors,
                InterfaceLive,
            ],
            // NetBox changes a fact's key (addresses, cables), not what the device says
            // its update jobs drop what they change
            JobKind::NetboxWrite | JobKind::NetboxUndo | JobKind::SiteDiff | JobKind::Rollout => {
                &[]
            }
        }
    }
}

/// What a setup changes on the neighbor it works through: what that sees on its port, and the
/// login the setup let through with a password for a moment.
const THROUGH_INVALIDATES: &[FactKind] = &[
    FactKind::Neighbors,
    FactKind::InterfaceLive,
    FactKind::Login,
];

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// What a job works on: a device, which runs one job at a time, or a site, whose job only reads
/// its devices and so keeps none of them from a job of its own, or the network (a rollout),
/// whose job starts a job of each device instead of working on it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JobSubject {
    Device(DeviceId),
    Site(SiteId),
    Network,
}

pub struct Job {
    id: u32,
    subject: JobSubject,
    /// the device's or the site's name
    name: Box<str>,
    kind: JobKind,
    /// another device the job works through and keeps from jobs of its own meanwhile: a
    /// setup's neighbor, whose console runs `/tool mac-telnet`
    through: Option<JobSubject>,
    log: Log,
    outcome: Mutex<Outcome>,
    took: Mutex<Option<Duration>>,
    started: tokio::time::Instant,
}

struct Jobs {
    next: u32,
    /// oldest first
    jobs: Vec<Arc<Job>>,
}

/// A change of a job's state ([`Log::unexpected`]).
#[derive(Debug, Clone, Copy)]
pub struct Transition {
    /// `None`: none yet
    pub from: Option<JobPhase>,
    pub to: JobPhase,
}

/// How a job stands or ended ([`Job::outcome`]).
#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: JobState,
    /// the error it failed with
    pub error: Option<ErrorInfo>,
}

/// Every lock here (the jobs, a job's outcome and events) takes a poisoned mutex's data as it
/// is: each holds plain values no half-done step can leave inconsistent, and a panic in one
/// request must not make every later job and the server panic too.
static JOBS: LazyLock<Mutex<Jobs>> = LazyLock::new(|| {
    Mutex::new(Jobs {
        next: first_id(SystemTime::now()),
        jobs: Vec::new(),
    })
});

/// Where the numbers of the jobs of a run start: 1000 per second of the start time, so the
/// numbers of two runs only meet after 1000 jobs per second between their starts (they wrap
/// every 11.5 days, `u32` and GraphQL's `Int` hold them).
fn first_id(started: SystemTime) -> u32 {
    let seconds = started
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (seconds % 1_000_000) as u32 * 1000
}

impl Job {
    pub fn job_kind(&self) -> JobKind {
        self.kind
    }
    /// The device it works on; `None` for a site's job.
    pub fn job_device(&self) -> Option<DeviceId> {
        match self.subject {
            JobSubject::Device(device) => Some(device),
            JobSubject::Site(_) | JobSubject::Network => None,
        }
    }
    pub fn is_running(&self) -> bool {
        self.state() == JobState::Running
    }
    /// Whether the job works on `subject` or through it.
    fn occupies(&self, subject: JobSubject) -> bool {
        self.subject == subject || self.through == Some(subject)
    }
    /// What the job wrote to NetBox (a `NetboxWrite` job), for taking it back.
    pub fn netbox_written(&self) -> Vec<helper::NetboxWritten> {
        self.log
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::NetboxWritten(written) => Some(written),
                _ => None,
            })
            .collect()
    }
    fn state(&self) -> JobState {
        self.outcome.lock_anyway().state
    }
    /// The state it is in, or once over the one its own work ended in ([`Log::ended_in`]).
    pub fn shown_phase(&self) -> Option<JobPhase> {
        if self.state() == JobState::Running {
            self.log.phase()
        } else {
            self.log.ended_in()
        }
    }
    fn finish_with(&self, result: Result<(), Error>) {
        match result {
            Ok(()) => self.finish(JobState::Done, None),
            Err(Error::Cancelled) => self.finish(JobState::Cancelled, None),
            Err(error) => self.finish(JobState::Failed, Some(ErrorInfo::from(&error))),
        }
    }
    fn finish(&self, state: JobState, error: Option<ErrorInfo>) {
        *self.took.lock_anyway() = Some(self.log.started.elapsed());
        match &error {
            Some(error) => info!(
                "{}: job {} {state:?}: {}",
                self.name, self.id, error.message
            ),
            None => info!("{}: job {} {state:?}", self.name, self.id),
        }
        *self.outcome.lock_anyway() = Outcome { state, error };
        // what the job may have changed is asked anew; only now, what was asked while it ran
        // isn't kept either (`facts`)
        if let JobSubject::Device(device) = self.subject {
            facts::invalidate(device, self.kind.invalidates());
        }
        if let Some(JobSubject::Device(through)) = self.through {
            facts::invalidate(through, THROUGH_INVALIDATES);
        }
        if let (JobSubject::Device(device), JobState::Done) = (self.subject, state) {
            self.keep_in_line(device);
        }
        self.log.changed();
    }
    /// An apply whose check found the device in line, with no fallback left or fired after it,
    /// keeps that as what is known of its diff (`site_diff::known`): „Abweichung von NetBox“
    /// shows it without reading the device again.
    fn keep_in_line(&self, device: DeviceId) {
        if self.kind != JobKind::Apply {
            return;
        }
        let checked = {
            let events = self.log.events.lock_anyway();
            let last = events
                .iter()
                .rposition(|logged| matches!(logged.event, Event::ApplyChecked(_)));
            last.and_then(|index| {
                let after = &events[index + 1..];
                let fell_back = after.iter().any(|logged| {
                    matches!(
                        logged.event,
                        Event::FallbackLeft(_) | Event::FallbackFired(_)
                    )
                });
                match &events[index].event {
                    Event::ApplyChecked(checked)
                        if checked.error.is_none()
                            && checked.remaining.is_empty()
                            && !fell_back =>
                    {
                        Some(Duration::from(events[index].at))
                    }
                    _ => None,
                }
            })
        };
        if let Some(at) = checked {
            let as_of = tokio::time::Instant::from_std(self.log.started + at);
            let key = facts::FactKey::new(
                FactKind::Config,
                Some(device),
                format!("checked by job {}", self.id),
            );
            facts::put(key, GeneratedCfg::in_line(as_of), as_of);
        }
    }
    pub fn job_id(&self) -> u32 {
        self.id
    }
    /// the device's or the site's name
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn log(&self) -> &Log {
        &self.log
    }
    /// how it stands or ended, with the error it failed with
    pub fn outcome(&self) -> Outcome {
        self.outcome.lock_anyway().clone()
    }
    /// how long it ran, once ended
    pub fn ran_for(&self) -> Option<Duration> {
        *self.took.lock_anyway()
    }
}

/// Starts `work` for `device` in the background, unless the device runs a job already.
pub fn start<F>(
    device: &DeviceAccess,
    kind: JobKind,
    started: Started,
    work: impl FnOnce(Log) -> F,
) -> Result<Arc<Job>, Error>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    start_on(device, None, kind, started, work)
}

/// Like [`start`], working through `through` as well (a setup over its neighbor): neither
/// device may run a job meanwhile, and `through`'s page lists it as its own.
pub fn start_through<F>(
    device: &DeviceAccess,
    through: &DeviceAccess,
    kind: JobKind,
    started: Started,
    work: impl FnOnce(Log) -> F,
) -> Result<Arc<Job>, Error>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    start_on(device, Some(through), kind, started, work)
}

fn start_on<F>(
    device: &DeviceAccess,
    through: Option<&DeviceAccess>,
    kind: JobKind,
    started: Started,
    work: impl FnOnce(Log) -> F,
) -> Result<Arc<Job>, Error>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let log = Log::new(device.name());
    let work = work(log.clone());
    let member = Member {
        of: subject_of(device),
        log,
    };
    let job = register(&[member], through.map(subject_of), kind)?.swap_remove(0);
    write_journal(&job, &started);
    spawn(job.clone(), work);
    Ok(job)
}

/// Starts `work` for `site` in the background (`JobKind::SiteDiff`). A site runs one job of a
/// kind at a time: while one runs, that one is given back instead of a second, so a page
/// opened again finds it.
pub fn start_site<F>(site: &SiteAccess, kind: JobKind, work: impl FnOnce(Log) -> F) -> Arc<Job>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let mut jobs = JOBS.lock_anyway();
    let subject = JobSubject::Site(site.id());
    if let Some(running) = jobs
        .jobs
        .iter()
        .find(|job| job.subject == subject && job.kind == kind && job.state() == JobState::Running)
    {
        return running.clone();
    }
    let log = Log::new(site.name());
    let work = work(log.clone());
    let job = add(&mut jobs, subject, None, site.name(), kind, log);
    drop(jobs);
    spawn(job.clone(), work);
    job
}

/// Starts `work` for the network (`JobKind::Rollout`), named `name`. The network runs one job
/// of a kind at a time, as a site does ([`start_site`]).
pub fn start_network<F>(name: &str, kind: JobKind, work: impl FnOnce(Log) -> F) -> Arc<Job>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let mut jobs = JOBS.lock_anyway();
    if let Some(running) = jobs.jobs.iter().find(|job| {
        job.subject == JobSubject::Network && job.kind == kind && job.state() == JobState::Running
    }) {
        return running.clone();
    }
    let log = Log::new(name);
    let work = work(log.clone());
    let job = add(&mut jobs, JobSubject::Network, None, name, kind, log);
    drop(jobs);
    spawn(job.clone(), work);
    job
}

/// The newest job of the network of `kind`, running or not.
pub fn newest_of_network(kind: JobKind) -> Option<Arc<Job>> {
    list(Some(JobSubject::Network))
        .into_iter()
        .find(|job| job.kind == kind)
}

/// The runtime the jobs run on. Started from a request, a job spawned on the runtime at hand
/// would run on the worker's, which ends with the server: dropped there, it would neither come
/// to its end nor to a state that goes on at the [`shutdown`].
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("jobs")
        .enable_all()
        .build()
        .expect("the jobs' runtime")
});

fn spawn<F>(job: Arc<Job>, work: F)
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    RUNTIME.spawn(async move {
        let result = run(work).await;
        job.log.ended();
        job.finish_with(result);
    });
}

/// Has the journal entry of a device's job written from now on, if entries are written.
fn write_journal(job: &Arc<Job>, started: &Started) {
    let JobSubject::Device(device) = job.subject else {
        return;
    };
    if !journal::enabled() {
        return;
    }
    let (changes, receiver) = watch::channel(());
    if job.log.journal.set(changes).is_ok() {
        RUNTIME.spawn(journal::follow(
            job.clone(),
            device,
            started.clone(),
            receiver,
        ));
    }
}

/// What a job is of, with its name.
#[derive(Clone, Copy)]
struct Named<'a> {
    subject: JobSubject,
    name: &'a str,
}

/// A job to [`register`].
struct Member<'a> {
    of: Named<'a>,
    log: Log,
}

fn subject_of(device: &DeviceAccess) -> Named<'_> {
    Named {
        subject: JobSubject::Device(device.id()),
        name: device.name(),
    }
}

/// Starts `work` for all of `devices` together (a batch), each with a job of its own (`started`:
/// who started the one of the device at that index), unless one of them runs a job already. `work` gets their logs in the order of `devices` and gives
/// back each job's outcome in that order.
pub fn start_all<F>(
    devices: &[DeviceAccess],
    kind: JobKind,
    started: impl Fn(usize) -> Started,
    work: impl FnOnce(Vec<Log>) -> F,
) -> Result<Vec<Arc<Job>>, Error>
where
    F: Future<Output = Vec<Result<(), Error>>> + Send + 'static,
{
    let logs: Vec<Log> = devices
        .iter()
        .map(|device| Log::new(device.name()))
        .collect();
    let work = work(logs.clone());
    let members: Vec<_> = devices
        .iter()
        .zip(logs)
        .map(|(device, log)| Member {
            of: subject_of(device),
            log,
        })
        .collect();
    let jobs = register(&members, None, kind)?;
    jobs.iter()
        .enumerate()
        .for_each(|(index, job)| write_journal(job, &started(index)));
    let running = jobs.clone();
    RUNTIME.spawn(async move {
        let result = run(async move { Ok(work.await) }).await;
        running.iter().for_each(|job| job.log.ended());
        match result {
            Ok(results) => {
                let mut results = results.into_iter();
                for job in &running {
                    job.finish_with(results.next().unwrap_or(Ok(())));
                }
            }
            Err(Error::Cancelled) => running
                .iter()
                .for_each(|job| job.finish(JobState::Cancelled, None)),
            Err(error) => {
                let error = ErrorInfo::from(&error);
                for job in &running {
                    job.finish(JobState::Failed, Some(error.clone()));
                }
            }
        }
    });
    Ok(jobs)
}

/// Adds a running job for each of `members`, none if one of their devices runs a job already.
fn register(
    members: &[Member],
    through: Option<Named>,
    kind: JobKind,
) -> Result<Vec<Arc<Job>>, Error> {
    let mut jobs = JOBS.lock_anyway();
    for Named { subject, name } in members.iter().map(|member| member.of).chain(through) {
        if jobs
            .jobs
            .iter()
            .any(|job| job.occupies(subject) && job.is_running())
        {
            return Err(Error::Busy(name.into()));
        }
    }
    let through = through.map(|through| through.subject);
    Ok(members
        .iter()
        .map(|member| {
            let Named { subject, name } = member.of;
            add(&mut jobs, subject, through, name, kind, member.log.clone())
        })
        .collect())
}

/// Adds a running job, dropping the oldest finished one beyond [`KEEP`].
fn add(
    jobs: &mut Jobs,
    subject: JobSubject,
    through: Option<JobSubject>,
    name: &str,
    kind: JobKind,
    log: Log,
) -> Arc<Job> {
    jobs.next += 1;
    let job = Arc::new(Job {
        id: jobs.next,
        subject,
        name: name.into(),
        kind,
        through,
        log,
        outcome: Mutex::new(Outcome {
            state: JobState::Running,
            error: None,
        }),
        took: Mutex::new(None),
        started: tokio::time::Instant::now(),
    });
    let devices = [Some(subject), through]
        .into_iter()
        .flatten()
        .filter_map(|subject| match subject {
            JobSubject::Device(device) => Some(device),
            JobSubject::Site(_) | JobSubject::Network => None,
        })
        .collect();
    let _ = job.log.devices.set(devices);
    job.log.announce();
    jobs.jobs.push(job.clone());
    let finished = jobs
        .jobs
        .iter()
        .filter(|job| job.state() != JobState::Running)
        .count();
    if finished > KEEP
        && let Some(oldest) = jobs
            .jobs
            .iter()
            .position(|job| job.state() != JobState::Running)
    {
        jobs.jobs.remove(oldest);
    }
    info!("{}: job {} {kind:?} started", job.name, job.id);
    job
}

/// Runs `work`; a panic in it is an error, instead of a job left running forever.
pub(crate) async fn run<T: Send + 'static>(
    work: impl Future<Output = Result<T, Error>> + Send + 'static,
) -> Result<T, Error> {
    tokio::spawn(work).await.unwrap_or_else(|join| {
        if !join.is_panic() {
            return Err(Error::Cancelled);
        }
        let panic = join.into_panic();
        let message = panic
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(Error::Panic(message.into()))
    })
}

pub fn get(id: u32) -> Option<Arc<Job>> {
    JOBS.lock_anyway()
        .jobs
        .iter()
        .find(|job| job.id == id)
        .cloned()
}

/// The jobs of `subject` (all without), newest first, with those working through it.
pub fn list(subject: Option<JobSubject>) -> Vec<Arc<Job>> {
    JOBS.lock_anyway()
        .jobs
        .iter()
        .rev()
        .filter(|job| subject.is_none_or(|subject| job.occupies(subject)))
        .cloned()
        .collect()
}

/// [`list`] for a GraphQL field: a subscription showing them answers again when they change.
pub fn list_shown(subject: Option<JobSubject>) -> Vec<Arc<Job>> {
    facts::note_jobs(match subject {
        Some(JobSubject::Device(device)) => Some(device),
        _ => None,
    });
    list(subject)
}

/// The running job of another device (an update, an apply changing a radio) that holds the
/// watchdogs of `device` suspended: restoring them meanwhile could reboot `device` while the
/// other one is away.
pub fn holding_watchdogs(device: &str) -> Option<WatchdogsHeld> {
    list(None)
        .into_iter()
        .filter(|job| job.is_running())
        .find(|job| holds_watchdogs(&job.log.events(), device))
        .map(|job| WatchdogsHeld {
            job: job.id,
            device: job.name.clone(),
        })
}

/// Whether `events` suspended the watchdogs of `device` and didn't restore them yet (the job
/// does at its end, or leaves them to the safety net).
fn holds_watchdogs(events: &[Event], device: &str) -> bool {
    events.iter().fold(false, |held, event| match event {
        Event::WatchdogSuspended(suspended) if *suspended.device == *device => true,
        Event::WatchdogRestored(restored) if *restored.device == *device => false,
        Event::WatchdogNotRestored(failed) if *failed.device == *device => false,
        Event::WatchdogKept(kept) if *kept.device == *device => false,
        Event::WatchdogStillHeld(held) if *held.device == *device => false,
        _ => held,
    })
}

/// The job holding a device's watchdogs suspended (`holding_watchdogs`).
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct WatchdogsHeld {
    pub job: u32,
    /// the device the job works on
    pub device: Box<str>,
}

/// The RouterOS `device` ran before the newest update of this run that brought another
/// version, for a downgrade back to it.
pub fn version_before_update(device: DeviceId) -> Option<Box<str>> {
    list(Some(JobSubject::Device(device)))
        .into_iter()
        .filter(|job| job.kind == JobKind::Update)
        .find_map(|job| {
            let events = job.log.events();
            let from = events.iter().find_map(|event| match event {
                Event::UpdateStart(start) => Some(start.version.clone()),
                _ => None,
            })?;
            events
                .iter()
                .any(|event| matches!(event, Event::RouterOsRunning(running) if running.version != from))
                .then_some(from)
        })
}

/// Asks the job to stop; its state turns `CANCELLED` once it got to a point where it can.
/// The server shuts down ([`shutdown`]).
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Lets the server shut down in order (`docs/jobs.md`, „Beenden“): a job whose file is written
/// stays as it is and goes on after the restart; every other is cancelled, and gets up to
/// `wait` to come to its end, or to a state that goes on (an apply cancelled while it starts
/// its script). Called once the server takes no more requests.
pub async fn shutdown(wait: Duration) {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let running: Vec<Arc<Job>> = JOBS
        .lock_anyway()
        .jobs
        .iter()
        .filter(|job| job.state() == JobState::Running)
        .cloned()
        .collect();
    for job in running.iter().filter(|job| !job.log.goes_on()) {
        info!("{}: job {} cancelled for the shutdown", job.name, job.id);
        job.log.cancel();
    }
    let deadline = Instant::now() + wait;
    let settled = || {
        running
            .iter()
            .all(|job| job.state() != JobState::Running || job.log.goes_on())
    };
    while !settled() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    for job in running
        .iter()
        .filter(|job| job.state() == JobState::Running)
    {
        match job.log.goes_on() {
            true => info!("{}: job {} goes on after the restart", job.name, job.id),
            false => warn!("{}: job {} still running at the shutdown", job.name, job.id),
        }
    }
}

pub fn cancel(id: u32) -> Option<Arc<Job>> {
    let job = get(id)?;
    job.log.cancel();
    Some(job)
}

#[Object]
impl Job {
    async fn id(&self) -> u32 {
        self.id
    }
    async fn kind(&self) -> JobKind {
        self.kind
    }
    /// seconds since it started
    async fn age(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
    /// the device it works on; `null` for a site's job
    async fn device_id(&self) -> Option<u32> {
        self.job_device().map(|device| device.0)
    }
    async fn device_name(&self) -> Option<&str> {
        self.job_device().map(|_| &*self.name)
    }
    /// the site it works on (`SITE_DIFF`); `null` for a device's job
    async fn site_id(&self) -> Option<u32> {
        match self.subject {
            JobSubject::Site(site) => Some(site.0),
            JobSubject::Device(_) | JobSubject::Network => None,
        }
    }
    async fn site_name(&self) -> Option<&str> {
        matches!(self.subject, JobSubject::Site(_)).then_some(&*self.name)
    }
    /// How many NetBox objects it wrote (a `NETBOX_WRITE`): what `undoNetboxWrite` takes back.
    /// One that failed midway has written the steps before.
    async fn netbox_written_count(&self) -> usize {
        self.netbox_written().len()
    }
    #[graphql(name = "state")]
    async fn graphql_state(&self) -> JobState {
        self.state()
    }
    /// why it failed
    async fn error(&self) -> Option<ErrorInfo> {
        self.outcome.lock_anyway().error.clone()
    }
    /// the state it is in (`device_state`), or the one its own work ended in (not restoring
    /// the watchdogs after it); `null` before its first and for a site's job
    async fn phase(&self) -> Option<crate::device_state::JobPhaseObject> {
        self.shown_phase().map(Into::into)
    }
    /// how long it ran; `None` while it runs
    async fn took(&self) -> Option<ScalarDuration> {
        self.took.lock_anyway().map(ScalarDuration::from)
    }
    /// its events from `after` on (the `index` after the last one seen); in a subscription
    /// each answer only those not delivered yet
    async fn events(&self, after: Option<u32>) -> Vec<Logged> {
        let from = facts::events_from(self.id, after.unwrap_or_default());
        let events = self.log.since(from);
        facts::note_delivered(self.id, from + events.len() as u32);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Event, JobKind, JobState, Log, cancel, get, holds_watchdogs, list, start, start_through,
    };
    use crate::journal::Started;

    fn by() -> Started {
        Started::By("test".into())
    }
    use crate::{
        device::{
            reboot::{Reboot, RebootReason},
            watchdog::{WatchdogRestored, WatchdogSuspended, Watching},
        },
        topology::{Device, DeviceId, TopologyHolder, builder::TopologyBuilder},
    };
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn reboot() -> Event {
        Event::Reboot(Reboot {
            reason: RebootReason::Packages,
            timeout: Duration::from_secs(1).into(),
        })
    }

    /// A job failing before it restores the watchdogs ended in the state it failed in.
    #[test]
    fn a_job_ends_in_its_own_state_not_in_restoring() {
        use crate::device_state::{BootstrapState, JobPhase};
        let log = Log::new("test");
        for state in [
            BootstrapState::SuspendingWatchdogs,
            BootstrapState::CheckingNeighbor,
            BootstrapState::Console,
        ] {
            log.enter(state);
        }
        assert_eq!(
            log.ended_in(),
            Some(JobPhase::Bootstrap(BootstrapState::Console))
        );
        log.enter(BootstrapState::RestoringWatchdogs);
        assert_eq!(
            log.phase(),
            Some(JobPhase::Bootstrap(BootstrapState::RestoringWatchdogs))
        );
        assert_eq!(
            log.ended_in(),
            Some(JobPhase::Bootstrap(BootstrapState::Console))
        );
        assert!(log.unexpected().is_empty());
    }

    /// What an apply keeps is in its file only while it is in a state that goes on after a
    /// restart, with that state; the job's end removes it.
    #[test]
    fn a_job_is_saved_only_in_states_that_go_on() {
        use crate::{
            device_state::{ApplyState, JobPhase},
            saved_job::{SavedApply, SavedJob, SavedWork, ScriptRun},
        };
        use std::time::SystemTime;
        let log = Log::new("lab-ap1");
        let state = |log: &Log| log.saved().map(|saved| saved.work.phase());
        log.enter(ApplyState::Planning);
        log.enter(ApplyState::ArmingFallback);
        log.enter(ApplyState::WritingScript);
        log.enter(ApplyState::StartingScript);
        log.keep(SavedJob {
            device: DeviceId(7),
            device_name: "lab-ap1".into(),
            site: None,
            saved_at: SystemTime::now(),
            journal_entry: None,
            work: SavedWork::Apply {
                state: ApplyState::ScriptRunning,
                apply: Box::new(SavedApply {
                    credentials: Some("lab".into()),
                    script: ScriptRun {
                        run: "a1b2c3d4".into(),
                        armed: true,
                        armed_at: SystemTime::now(),
                        fires: SystemTime::now(),
                        unmarked: SystemTime::now(),
                        reprovision: Box::default(),
                    },
                    hold: None,
                    batch: None,
                }),
            },
        });
        assert_eq!(state(&log), None);
        log.enter(ApplyState::ScriptRunning);
        assert_eq!(
            state(&log),
            Some(JobPhase::Apply(ApplyState::ScriptRunning))
        );
        log.enter(ApplyState::StoppingFallback);
        assert_eq!(
            state(&log),
            Some(JobPhase::Apply(ApplyState::StoppingFallback))
        );
        log.enter(ApplyState::ScriptRunning);
        assert_eq!(
            state(&log),
            Some(JobPhase::Apply(ApplyState::ScriptRunning))
        );
        log.enter(ApplyState::StoppingFallback);
        log.enter(ApplyState::Checking);
        assert_eq!(state(&log), None);
        log.enter(ApplyState::Finishing);
        assert_eq!(state(&log), None);
        assert!(log.unexpected().is_empty());

        // saved before the restart: written again as entered then
        let resumed = Log::new("lab-ap1");
        let mut before = log.saving.lock().unwrap().job.clone().unwrap();
        before.saved_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        resumed.keep(before.clone());
        resumed.resume_in(ApplyState::ScriptRunning);
        assert_eq!(
            state(&resumed),
            Some(JobPhase::Apply(ApplyState::ScriptRunning))
        );
        assert_eq!(
            resumed.saved().map(|saved| saved.saved_at),
            Some(before.saved_at)
        );
        resumed.ended();
        assert_eq!(state(&resumed), None);
    }

    /// A job going on after a restart may enter the state it was saved in once more (an update
    /// saved in `NotUpdated` decides about its fallback again), but no other self-loop.
    #[test]
    fn a_resumed_job_enters_its_state_again() {
        use crate::device_state::UpdateState;
        let log = Log::new("lab-ap1");
        log.resume_in(UpdateState::NotUpdated);
        log.enter(UpdateState::NotUpdated);
        assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
        let log = Log::new("lab-ap1");
        log.resume_in(UpdateState::StoppingFallback);
        log.enter(UpdateState::StoppingFallback);
        log.enter(UpdateState::WaitingForNeighbors);
        log.enter(UpdateState::WaitingForNeighbors);
        assert_eq!(log.unexpected().len(), 1);
    }

    #[test]
    fn hands_out_the_events_after_an_index() {
        let log = Log::new("t");
        log.emit(reboot());
        log.emit(reboot());
        assert_eq!(log.since(0).len(), 2);
        assert_eq!(log.since(1)[0].index, 1);
        assert!(log.since(2).is_empty());
        assert!(log.since(5).is_empty());
    }

    #[test]
    fn numbers_of_two_runs_are_apart() {
        use super::first_id;
        use std::time::{SystemTime, UNIX_EPOCH};
        let start = UNIX_EPOCH + Duration::from_secs(1_791_000_000);
        let restart = start + Duration::from_secs(30);
        assert_eq!(first_id(restart) - first_id(start), 30_000);
        // it wraps instead of overflowing
        assert!(first_id(SystemTime::now()) < 1_000_000_000);
    }

    async fn wait_for(id: u32, state: JobState) {
        timeout(Duration::from_secs(1), async {
            while get(id).unwrap().state() != state {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job state");
    }

    #[test]
    fn a_panic_holding_the_events_does_not_stop_the_log() {
        let log = Log::new("poisoned");
        let events = log.events.clone();
        std::thread::spawn(move || {
            let _held = events.lock().unwrap();
            panic!("while holding the events");
        })
        .join()
        .unwrap_err();
        assert!(log.events.is_poisoned());
        log.emit(reboot());
        assert_eq!(log.events(), [reboot()]);
    }

    #[tokio::test]
    async fn runs_one_job_per_device_and_cancels_it() {
        let mut builder = TopologyBuilder::default();
        let device = builder.devices.next_id();
        builder.devices.insert(
            device,
            Device {
                name: "job-test".into(),
                ..Default::default()
            },
        );
        let topology: TopologyHolder = builder.into();
        let device = topology.devices_by_id(device).await.unwrap();

        let job = start(&device, JobKind::Update, by(), |log| async move {
            log.emit(reboot());
            log.unless_cancelled(sleep(Duration::from_secs(60))).await?;
            Ok(())
        })
        .unwrap();
        assert!(start(&device, JobKind::Update, by(), |_| async { Ok(()) }).is_err());
        // cancelled while it waits
        while job.log.events().is_empty() {
            sleep(Duration::from_millis(10)).await;
        }
        cancel(job.id);
        wait_for(job.id, JobState::Cancelled).await;
        assert_eq!(job.log.events(), [reboot()]);

        let done = start(&device, JobKind::Update, by(), |_| async { Ok(()) }).unwrap();
        wait_for(done.id, JobState::Done).await;

        // a bug in the work: failed with the panic's message, and the device is free again
        let panicked = start(&device, JobKind::Apply, by(), |_| async {
            if true {
                panic!("Send command failed");
            }
            Ok(())
        })
        .unwrap();
        wait_for(panicked.id, JobState::Failed).await;
        let error = panicked
            .outcome
            .lock()
            .unwrap()
            .error
            .clone()
            .expect("error");
        assert_eq!(error.code, "PANIC");
        assert!(
            error.message.contains("Send command failed"),
            "{}",
            error.message
        );
        let after = start(&device, JobKind::Update, by(), |_| async { Ok(()) }).unwrap();
        wait_for(after.id, JobState::Done).await;
    }

    #[test]
    fn watchdogs_are_held_from_suspended_to_restored() {
        let watching = Watching {
            watch_address: Some("10.0.0.1".into()),
            poe_pings: Vec::new(),
        };
        let suspended = |device: &str| {
            Event::WatchdogSuspended(WatchdogSuspended {
                device: device.into(),
                watching: watching.clone(),
                safety_net: Duration::from_secs(7200).into(),
            })
        };
        let restored = |device: &str| {
            Event::WatchdogRestored(WatchdogRestored {
                device: device.into(),
                watching: watching.clone(),
            })
        };
        assert!(!holds_watchdogs(&[], "sw1"));
        assert!(holds_watchdogs(&[reboot(), suspended("sw1")], "sw1"));
        assert!(!holds_watchdogs(&[suspended("sw2")], "sw1"));
        assert!(!holds_watchdogs(
            &[suspended("sw1"), restored("sw1")],
            "sw1"
        ));
        assert!(holds_watchdogs(
            &[suspended("sw1"), suspended("sw2"), restored("sw2")],
            "sw1"
        ));
    }

    #[tokio::test]
    async fn a_setup_keeps_its_neighbor_busy() {
        let mut builder = TopologyBuilder::default();
        // apart from the other tests' devices: the jobs are global
        let ids = [DeviceId(9001), DeviceId(9002)];
        for (id, name) in ids.into_iter().zip(["setup-new", "setup-neighbor"]) {
            builder.devices.insert(
                id,
                Device {
                    name: name.into(),
                    ..Default::default()
                },
            );
        }
        let topology: TopologyHolder = builder.into();
        let new = topology.devices_by_id(ids[0]).await.unwrap();
        let neighbor = topology.devices_by_id(ids[1]).await.unwrap();

        let job = start_through(
            &new,
            &neighbor,
            JobKind::Bootstrap,
            by(),
            |log| async move {
                log.unless_cancelled(sleep(Duration::from_secs(60))).await?;
                Ok(())
            },
        )
        .unwrap();
        assert!(start(&neighbor, JobKind::Apply, by(), |_| async { Ok(()) }).is_err());
        assert!(start(&new, JobKind::Apply, by(), |_| async { Ok(()) }).is_err());
        // the neighbor's page finds it
        let subject = super::subject_of(&neighbor).subject;
        assert!(list(Some(subject)).iter().any(|listed| listed.id == job.id));
        cancel(job.id);
        wait_for(job.id, JobState::Cancelled).await;
        start(&neighbor, JobKind::Apply, by(), |_| async { Ok(()) }).unwrap();
    }
}
