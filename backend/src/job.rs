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
        bootstrap, device_mode, partitions, power, ros::graphql as apply, terminal, update,
        update_fallback,
    },
    graphql::scalars::ScalarDuration,
    topology::{DeviceId, access::device::DeviceAccess},
};
use async_graphql::{Enum, Object, SimpleObject, Union};
use log::info;
use std::{
    sync::{
        Arc, LazyLock, Mutex,
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
    PoeChain(power::PoeChain),
    WaitingForPower(power::WaitingForPower),
    UpdateStart(update::UpdateStart),
    UpdateStepPlanned(update::UpdateStepPlanned),
    SshPasswordLogin(update::SshPasswordLogin),
    PackageDownloaded(update::PackageDownloaded),
    PackageUploaded(update::PackageUploaded),
    Reboot(update::Reboot),
    RouterOsRunning(update::RouterOsRunning),
    FirmwareUpgrade(update::FirmwareUpgrade),
    UpdateDone(update::UpdateDone),
    ApplyPlanned(apply::ApplyPlanned),
    FallbackArmed(apply::FallbackArmed),
    ApplyRunning(apply::ApplyRunning),
    ApplyChecked(apply::ApplyChecked),
    FallbackStopped(apply::FallbackStopped),
    FallbackLeft(apply::FallbackLeft),
    RadiosReprovisioned(apply::RadiosReprovisioned),
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
        let mut events = self.events.lock().expect("job events");
        let index = events.len() as u32;
        events.push(Logged {
            index,
            at: self.started.elapsed().into(),
            event,
        });
    }
    /// The events from `after` on.
    pub fn since(&self, after: u32) -> Vec<Logged> {
        let events = self.events.lock().expect("job events");
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
    /// the generated config (`applyCfg`)
    Apply,
    /// initial setup over mac-telnet (`bootstrap`)
    Bootstrap,
    /// a reboot asked for on the device page (`rebootDevice`)
    Reboot,
    /// features turned on in the device mode (`changeDeviceMode`)
    DeviceMode,
    /// the flash split in partitions (`repartitionDevice`)
    Repartition,
    /// booting from another partition (`bootPartition`)
    BootPartition,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

pub struct Job {
    id: u32,
    device: DeviceId,
    device_name: Box<str>,
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
    fn state(&self) -> JobState {
        self.outcome.lock().expect("job outcome").0
    }
    fn finish(&self, state: JobState, error: Option<ErrorInfo>) {
        *self.took.lock().expect("job time") = Some(self.log.started.elapsed());
        match &error {
            Some(error) => info!("{}: job {} {state:?}: {}", self.device_name, self.id, error.message),
            None => info!("{}: job {} {state:?}", self.device_name, self.id),
        }
        *self.outcome.lock().expect("job outcome") = (state, error);
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
    let job = {
        let mut jobs = JOBS.lock().expect("jobs");
        if jobs
            .jobs
            .iter()
            .any(|job| job.device == device.id() && job.state() == JobState::Running)
        {
            return Err(Error::Busy(device.name().into()));
        }
        jobs.next += 1;
        let job = Arc::new(Job {
            id: jobs.next,
            device: device.id(),
            device_name: device.name().into(),
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
        job
    };
    info!("{}: job {} {kind:?} started", job.device_name, job.id);
    let running = job.clone();
    tokio::spawn(async move {
        // a panic in the work ends the job as failed, instead of leaving it running forever
        let result = tokio::spawn(work).await.unwrap_or_else(|join| {
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
        });
        match result {
            Ok(()) => running.finish(JobState::Done, None),
            Err(Error::Cancelled) => running.finish(JobState::Cancelled, None),
            Err(error) => running.finish(JobState::Failed, Some(ErrorInfo::from(&error))),
        }
    });
    Ok(job)
}

pub fn get(id: u32) -> Option<Arc<Job>> {
    JOBS.lock()
        .expect("jobs")
        .jobs
        .iter()
        .find(|job| job.id == id)
        .cloned()
}

/// The jobs of `device` (all without), newest first.
pub fn list(device: Option<DeviceId>) -> Vec<Arc<Job>> {
    JOBS.lock()
        .expect("jobs")
        .jobs
        .iter()
        .rev()
        .filter(|job| device.is_none_or(|device| job.device == device))
        .cloned()
        .collect()
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
    async fn device_id(&self) -> u32 {
        self.device.0
    }
    async fn device_name(&self) -> &str {
        &self.device_name
    }
    #[graphql(name = "state")]
    async fn graphql_state(&self) -> JobState {
        self.state()
    }
    /// why it failed
    async fn error(&self) -> Option<ErrorInfo> {
        self.outcome.lock().expect("job outcome").1.clone()
    }
    /// how long it ran; `None` while it runs
    async fn took(&self) -> Option<ScalarDuration> {
        self.took.lock().expect("job time").map(ScalarDuration::from)
    }
    /// its events from `after` on (the `index` after the last one seen)
    async fn events(&self, after: Option<u32>) -> Vec<Logged> {
        self.log.since(after.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, JobKind, JobState, Log, cancel, get, start};
    use crate::{
        device::update::{Reboot, RebootReason},
        topology::{Device, TopologyHolder, test::TopologyBuilder},
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
}
