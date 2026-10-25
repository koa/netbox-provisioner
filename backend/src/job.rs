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
    error_info::ErrorInfo,
    Error,
    device::{
        self, batch, bootstrap, device_mode, leftovers, partitions, power, power_cycle, ros::graphql as apply, terminal, update,
        radio_hold, update_fallback, watchdog,
    },
    graphql::scalars::ScalarDuration,
    helper,
    topology::{
        DeviceId, SiteId,
        access::{AccessTopology, device::DeviceAccess, site::SiteAccess},
    },
};
use async_graphql::{Enum, Object, SimpleObject, Union};
use log::info;
use std::{
    sync::{
        Arc, LazyLock, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Notify;

/// How many finished jobs are kept.
const KEEP: usize = 50;

/// Something a job did or waits for.
#[derive(Debug, Clone, PartialEq, Union)]
pub enum Event {
    Connecting(device::Connecting),
    Connected(device::Connected),
    PoeChain(power::PoeChain),
    WaitingForPower(power::WaitingForPower),
    UpdateTarget(update::UpdateTarget),
    UpdateStart(update::UpdateStart),
    UpdateStepPlanned(update::UpdateStepPlanned),
    SshPasswordLogin(update::SshPasswordLogin),
    PackageDownloaded(update::PackageDownloaded),
    PackageUploading(update::PackageUploading),
    PackageUploaded(update::PackageUploaded),
    Reboot(update::Reboot),
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
    UpdateFallbackArmed(update_fallback::UpdateFallbackArmed),
    UpdateFallbackDisarmed(update_fallback::UpdateFallbackDisarmed),
    UpdateWithoutFallback(update_fallback::UpdateWithoutFallback),
    OspfNeighborsBack(update_fallback::OspfNeighborsBack),
    WatchdogNotSet(watchdog::WatchdogNotSet),
    WatchdogSuspended(watchdog::WatchdogSuspended),
    WatchdogRestored(watchdog::WatchdogRestored),
    WatchdogNotRestored(watchdog::WatchdogNotRestored),
    WatchdogLeft(watchdog::WatchdogLeft),
    WatchdogKept(radio_hold::WatchdogKept),
    NetboxWritten(helper::NetboxWritten),
    NetboxUndone(helper::NetboxUndone),
    SiteDiffStarted(device::site_diff::SiteDiffStarted),
    DeviceDiffed(device::site_diff::DeviceDiffed),
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
}

impl Log {
    /// `label` names the device in the server's own log.
    pub fn new(label: &str) -> Log {
        Log {
            label: label.into(),
            started: Instant::now(),
            events: Default::default(),
            cancelled: Default::default(),
        }
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
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        let index = events.len() as u32;
        events.push(Logged {
            index,
            at: self.started.elapsed().into(),
            event,
        });
    }
    /// The events from `after` on.
    pub fn since(&self, after: u32) -> Vec<Logged> {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.get(after as usize..).unwrap_or_default().to_vec()
    }
    pub fn events(&self) -> Vec<Event> {
        self.since(0).into_iter().map(|logged| logged.event).collect()
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
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// What a job works on: a device, which runs one job at a time, or a site, whose job only reads
/// its devices and so keeps none of them from a job of its own.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JobSubject {
    Device(DeviceId),
    Site(SiteId),
}

pub struct Job {
    id: u32,
    subject: JobSubject,
    /// the device's or the site's name
    name: Box<str>,
    kind: JobKind,
    log: Log,
    outcome: Mutex<(JobState, Option<ErrorInfo>)>,
    took: Mutex<Option<Duration>>,
}

struct Jobs {
    next: u32,
    /// oldest first
    jobs: Vec<Arc<Job>>,
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
    let seconds = started.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
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
            JobSubject::Site(_) => None,
        }
    }
    pub fn is_running(&self) -> bool {
        self.state() == JobState::Running
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
        self.outcome.lock().unwrap_or_else(PoisonError::into_inner).0
    }
    fn finish_with(&self, result: Result<(), Error>) {
        match result {
            Ok(()) => self.finish(JobState::Done, None),
            Err(Error::Cancelled) => self.finish(JobState::Cancelled, None),
            Err(error) => self.finish(JobState::Failed, Some(ErrorInfo::from(&error))),
        }
    }
    fn finish(&self, state: JobState, error: Option<ErrorInfo>) {
        *self.took.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.log.started.elapsed());
        match &error {
            Some(error) => info!("{}: job {} {state:?}: {}", self.name, self.id, error.message),
            None => info!("{}: job {} {state:?}", self.name, self.id),
        }
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = (state, error);
    }
}

/// Starts `work` for `device` in the background, unless the device runs a job already.
pub fn start<F>(
    device: &DeviceAccess,
    kind: JobKind,
    work: impl FnOnce(Log) -> F,
) -> Result<Arc<Job>, Error>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let log = Log::new(device.name());
    let work = work(log.clone());
    let job = register(&[(subject_of(device), log)], kind)?.swap_remove(0);
    spawn(job.clone(), work);
    Ok(job)
}

/// Starts `work` for `site` in the background (`JobKind::SiteDiff`). A site runs one job of a
/// kind at a time: while one runs, that one is given back instead of a second, so a page
/// opened again finds it.
pub fn start_site<F>(
    site: &SiteAccess,
    kind: JobKind,
    work: impl FnOnce(Log) -> F,
) -> Arc<Job>
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let mut jobs = JOBS.lock().unwrap_or_else(PoisonError::into_inner);
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
    let job = add(&mut jobs, subject, site.name(), kind, log);
    drop(jobs);
    spawn(job.clone(), work);
    job
}

fn spawn<F>(job: Arc<Job>, work: F)
where
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    tokio::spawn(async move {
        let result = run(work).await;
        job.finish_with(result);
    });
}

fn subject_of(device: &DeviceAccess) -> (JobSubject, &str) {
    (JobSubject::Device(device.id()), device.name())
}

/// Starts `work` for all of `devices` together (a batch), each with a job of its own, unless
/// one of them runs a job already. `work` gets their logs in the order of `devices` and gives
/// back each job's outcome in that order.
pub fn start_all<F>(
    devices: &[DeviceAccess],
    kind: JobKind,
    work: impl FnOnce(Vec<Log>) -> F,
) -> Result<Vec<Arc<Job>>, Error>
where
    F: Future<Output = Vec<Result<(), Error>>> + Send + 'static,
{
    let logs: Vec<Log> = devices.iter().map(|device| Log::new(device.name())).collect();
    let work = work(logs.clone());
    let members: Vec<_> = devices.iter().map(subject_of).zip(logs).collect();
    let jobs = register(&members, kind)?;
    let running = jobs.clone();
    tokio::spawn(async move {
        match run(async move { Ok(work.await) }).await {
            Ok(results) => {
                let mut results = results.into_iter();
                for job in &running {
                    job.finish_with(results.next().unwrap_or(Ok(())));
                }
            }
            Err(Error::Cancelled) => running.iter().for_each(|job| job.finish(JobState::Cancelled, None)),
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
fn register(members: &[((JobSubject, &str), Log)], kind: JobKind) -> Result<Vec<Arc<Job>>, Error> {
    let mut jobs = JOBS.lock().unwrap_or_else(PoisonError::into_inner);
    for ((subject, name), _) in members {
        if jobs
            .jobs
            .iter()
            .any(|job| job.subject == *subject && job.state() == JobState::Running)
        {
            return Err(Error::Busy((*name).into()));
        }
    }
    Ok(members
        .iter()
        .map(|((subject, name), log)| add(&mut jobs, *subject, name, kind, log.clone()))
        .collect())
}

/// Adds a running job, dropping the oldest finished one beyond [`KEEP`].
fn add(jobs: &mut Jobs, subject: JobSubject, name: &str, kind: JobKind, log: Log) -> Arc<Job> {
    jobs.next += 1;
    let job = Arc::new(Job {
        id: jobs.next,
        subject,
        name: name.into(),
        kind,
        log,
        outcome: Mutex::new((JobState::Running, None)),
        took: Mutex::new(None),
    });
    jobs.jobs.push(job.clone());
    let finished = jobs.jobs.iter().filter(|job| job.state() != JobState::Running).count();
    if finished > KEEP
        && let Some(oldest) = jobs.jobs.iter().position(|job| job.state() != JobState::Running)
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
    JOBS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .jobs
        .iter()
        .find(|job| job.id == id)
        .cloned()
}

/// The jobs of `subject` (all without), newest first.
pub fn list(subject: Option<JobSubject>) -> Vec<Arc<Job>> {
    JOBS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .jobs
        .iter()
        .rev()
        .filter(|job| subject.is_none_or(|subject| job.subject == subject))
        .cloned()
        .collect()
}

/// The running job of another device (an update, an apply changing a radio) that holds the
/// watchdogs of `device` suspended: restoring them meanwhile could reboot `device` while the
/// other one is away.
pub fn holding_watchdogs(device: &str) -> Option<WatchdogsHeld> {
    list(None)
        .into_iter()
        .filter(|job| job.is_running())
        .find(|job| holds_watchdogs(&job.log.events(), device))
        .map(|job| WatchdogsHeld { job: job.id, device: job.name.clone() })
}

/// Whether `events` suspended the watchdogs of `device` and didn't restore them yet (the job
/// does at its end, or leaves them to the safety net).
fn holds_watchdogs(events: &[Event], device: &str) -> bool {
    events.iter().fold(false, |held, event| match event {
        Event::WatchdogSuspended(suspended) if *suspended.device == *device => true,
        Event::WatchdogRestored(restored) if *restored.device == *device => false,
        Event::WatchdogNotRestored(failed) if *failed.device == *device => false,
        Event::WatchdogKept(kept) if *kept.device == *device => false,
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
            JobSubject::Device(_) => None,
        }
    }
    async fn site_name(&self) -> Option<&str> {
        matches!(self.subject, JobSubject::Site(_)).then_some(&*self.name)
    }
    #[graphql(name = "state")]
    async fn graphql_state(&self) -> JobState {
        self.state()
    }
    /// why it failed
    async fn error(&self) -> Option<ErrorInfo> {
        self.outcome.lock().unwrap_or_else(PoisonError::into_inner).1.clone()
    }
    /// how long it ran; `None` while it runs
    async fn took(&self) -> Option<ScalarDuration> {
        self.took.lock().unwrap_or_else(PoisonError::into_inner).map(ScalarDuration::from)
    }
    /// its events from `after` on (the `index` after the last one seen)
    async fn events(&self, after: Option<u32>) -> Vec<Logged> {
        self.log.since(after.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, JobKind, JobState, Log, cancel, get, holds_watchdogs, start};
    use crate::{
        device::{
            update::{Reboot, RebootReason},
            watchdog::{WatchdogRestored, WatchdogSuspended, Watching},
        },
        topology::{Device, TopologyHolder, builder::TopologyBuilder},
    };
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn reboot() -> Event {
        Event::Reboot(Reboot {
            reason: RebootReason::Packages,
            timeout: Duration::from_secs(1).into(),
        })
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

        let job = start(&device, JobKind::Update, |log| async move {
            log.emit(reboot());
            log.unless_cancelled(sleep(Duration::from_secs(60))).await?;
            Ok(())
        })
        .unwrap();
        assert!(start(&device, JobKind::Update, |_| async { Ok(()) }).is_err());
        // cancelled while it waits
        while job.log.events().is_empty() {
            sleep(Duration::from_millis(10)).await;
        }
        cancel(job.id);
        wait_for(job.id, JobState::Cancelled).await;
        assert_eq!(job.log.events(), [reboot()]);

        let done = start(&device, JobKind::Update, |_| async { Ok(()) }).unwrap();
        wait_for(done.id, JobState::Done).await;

        // a bug in the work: failed with the panic's message, and the device is free again
        let panicked = start(&device, JobKind::Apply, |_| async {
            if true {
                panic!("Send command failed");
            }
            Ok(())
        })
        .unwrap();
        wait_for(panicked.id, JobState::Failed).await;
        let error = panicked.outcome.lock().unwrap().1.clone().expect("error");
        assert_eq!(error.code, "PANIC");
        assert!(error.message.contains("Send command failed"), "{}", error.message);
        let after = start(&device, JobKind::Update, |_| async { Ok(()) }).unwrap();
        wait_for(after.id, JobState::Done).await;
    }

    #[test]
    fn watchdogs_are_held_from_suspended_to_restored() {
        let watching = Watching { watch_address: Some("10.0.0.1".into()), poe_pings: Vec::new() };
        let suspended = |device: &str| {
            Event::WatchdogSuspended(WatchdogSuspended {
                device: device.into(),
                watching: watching.clone(),
                safety_net: Duration::from_secs(7200).into(),
            })
        };
        let restored = |device: &str| {
            Event::WatchdogRestored(WatchdogRestored { device: device.into(), watching: watching.clone() })
        };
        assert!(!holds_watchdogs(&[], "sw1"));
        assert!(holds_watchdogs(&[reboot(), suspended("sw1")], "sw1"));
        assert!(!holds_watchdogs(&[suspended("sw2")], "sw1"));
        assert!(!holds_watchdogs(&[suspended("sw1"), restored("sw1")], "sw1"));
        assert!(holds_watchdogs(&[suspended("sw1"), suspended("sw2"), restored("sw2")], "sw1"));
    }
}
