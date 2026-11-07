use super::{
    Control, RolloutDevice, RolloutDeviceState as State, RolloutDone, RolloutPause, RolloutPaused,
    UpdateJob, run,
};
use crate::{
    Error,
    device::{
        rollout::{PlannedDevice, RolloutFallback, RolloutPlan, RolloutSkip},
        update::Channel,
    },
    device_state::UpdateState,
    error_info::ErrorInfo,
    job::{Event, JobState, Log},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// How a device's update goes.
#[derive(Clone, Copy)]
enum Goes {
    Done,
    Fails,
    /// busy with another job for that many tries, then done
    Busy(u32),
    /// runs until cancelled
    Hangs,
}

#[derive(Clone)]
struct FakeJob {
    id: u32,
    state: JobState,
    cancelled: Arc<AtomicBool>,
    phase: Arc<Mutex<Option<UpdateState>>>,
}

impl UpdateJob for FakeJob {
    fn id(&self) -> u32 {
        self.id
    }
    fn outcome(&self) -> (JobState, Option<ErrorInfo>) {
        match self.cancelled.load(Ordering::SeqCst) {
            true => (JobState::Cancelled, None),
            false => (self.state, None),
        }
    }
    fn phase(&self) -> Option<UpdateState> {
        *self.phase.lock().unwrap()
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

struct Devices {
    goes: Mutex<HashMap<u32, Goes>>,
    cancelled: Arc<AtomicBool>,
    /// the state every update is in
    phase: Arc<Mutex<Option<UpdateState>>>,
}

impl Devices {
    fn new(goes: &[(u32, Goes)]) -> Arc<Self> {
        Arc::new(Devices {
            goes: Mutex::new(goes.iter().copied().collect()),
            cancelled: Arc::default(),
            phase: Arc::default(),
        })
    }
    async fn start(&self, device: u32) -> Result<FakeJob, Error> {
        let mut goes = self.goes.lock().unwrap();
        let state = match goes.get(&device).copied().unwrap_or(Goes::Done) {
            Goes::Done => JobState::Done,
            Goes::Fails => JobState::Failed,
            Goes::Hangs => JobState::Running,
            Goes::Busy(0) => JobState::Done,
            Goes::Busy(tries) => {
                goes.insert(device, Goes::Busy(tries - 1));
                return Err(Error::Busy("other job".into()));
            }
        };
        Ok(FakeJob {
            id: 1000 + device,
            state,
            cancelled: self.cancelled.clone(),
            phase: self.phase.clone(),
        })
    }
}

fn device(id: u32, pilot: bool) -> PlannedDevice {
    PlannedDevice {
        id,
        name: format!("d{id}").into(),
        site_id: 1,
        site: "site".into(),
        version: Some("7.23.8".into()),
        architecture: Some("arm".into()),
        files: Vec::new(),
        download: None,
        free_space: None,
        in_ram: false,
        fallback: RolloutFallback::Partition,
        skip: None,
        error: None,
        pilot,
        depth: Some(1),
        related: Vec::new(),
        cut_by: Vec::new(),
    }
}

fn plan(devices: Vec<PlannedDevice>) -> RolloutPlan {
    RolloutPlan {
        channel: Channel::LongTerm,
        target: "7.24.6".into(),
        devices,
    }
}

/// Runs `plan` in the background.
fn start(
    plan: RolloutPlan,
    devices: Arc<Devices>,
) -> (
    Log,
    Arc<Control>,
    tokio::task::JoinHandle<Result<(), Error>>,
) {
    let log = Log::new("rollout");
    let control = Arc::new(Control::default());
    let (running_log, running_control) = (log.clone(), control.clone());
    let handle = tokio::spawn(async move {
        run(plan, &running_control, &running_log, |device| {
            let devices = devices.clone();
            async move { devices.start(device).await }
        })
        .await
    });
    (log, control, handle)
}

/// Waits until the log has `count` pauses.
async fn pauses(log: &Log, count: usize) -> Vec<RolloutPaused> {
    loop {
        let paused = log
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::RolloutPaused(paused) => Some(paused),
                _ => None,
            })
            .collect::<Vec<_>>();
        if paused.len() >= count {
            return paused;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn states(log: &Log) -> Vec<(u32, State)> {
    log.events()
        .into_iter()
        .filter_map(|event| match event {
            Event::RolloutDevice(RolloutDevice { device, state, .. }) => Some((device, state)),
            _ => None,
        })
        .collect()
}

fn done(log: &Log) -> Option<RolloutDone> {
    log.events().into_iter().find_map(|event| match event {
        Event::RolloutDone(done) => Some(done),
        _ => None,
    })
}

#[tokio::test(start_paused = true)]
async fn the_rest_waits_for_the_pilots_to_be_seen() {
    let devices = Devices::new(&[]);
    let (log, control, handle) = start(
        plan(vec![device(1, true), device(2, false), device(3, false)]),
        devices,
    );
    let paused = pauses(&log, 1).await;
    assert_eq!(paused[0].reason, RolloutPause::AfterPilots);
    assert_eq!(states(&log), [(1, State::Started), (1, State::Done)]);
    control.go_on();
    handle.await.unwrap().unwrap();
    let done = done(&log).unwrap();
    assert_eq!((done.updated, done.skipped, done.failed), (3, 0, 0));
}

#[tokio::test(start_paused = true)]
async fn a_failed_device_pauses_and_stays_as_it_is() {
    let devices = Devices::new(&[(2, Goes::Fails)]);
    let mut failing = device(2, false);
    failing.depth = Some(3);
    let (log, control, handle) = start(plan(vec![failing, device(3, false)]), devices);
    let paused = pauses(&log, 1).await;
    assert_eq!(paused[0].reason, RolloutPause::Failed);
    assert_eq!(paused[0].device.as_deref(), Some("d2"));
    control.go_on();
    handle.await.unwrap().unwrap();
    let states = states(&log);
    assert!(states.contains(&(2, State::Failed)));
    assert!(states.contains(&(3, State::Done)));
    let done = done(&log).unwrap();
    assert_eq!((done.updated, done.skipped, done.failed), (1, 0, 1));
}

#[tokio::test(start_paused = true)]
async fn a_device_busy_with_another_job_waits_for_it() {
    let devices = Devices::new(&[(1, Goes::Busy(3))]);
    let (log, _, handle) = start(plan(vec![device(1, false)]), devices);
    handle.await.unwrap().unwrap();
    assert_eq!(
        states(&log),
        [(1, State::Waiting), (1, State::Started), (1, State::Done)]
    );
}

#[tokio::test(start_paused = true)]
async fn a_skipped_device_is_left_out_and_one_planned_out_never_comes() {
    let devices = Devices::new(&[]);
    let mut up_to_date = device(4, false);
    up_to_date.skip = Some(RolloutSkip::UpToDate);
    let (log, control, handle) = start(
        plan(vec![device(1, true), device(2, false), up_to_date]),
        devices,
    );
    pauses(&log, 1).await;
    control.skip(2);
    control.go_on();
    handle.await.unwrap().unwrap();
    assert_eq!(
        states(&log),
        [(1, State::Started), (1, State::Done), (2, State::Skipped)]
    );
    let done = done(&log).unwrap();
    assert_eq!((done.updated, done.skipped, done.failed), (1, 1, 0));
}

#[tokio::test(start_paused = true)]
async fn cancelled_it_cancels_the_running_updates() {
    let devices = Devices::new(&[(1, Goes::Hangs)]);
    let cancelled = devices.cancelled.clone();
    let (log, _, handle) = start(plan(vec![device(1, false), device(2, false)]), devices);
    while states(&log).len() < 2 {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    log.cancel();
    assert!(matches!(handle.await.unwrap(), Err(Error::Cancelled)));
    assert!(cancelled.load(Ordering::SeqCst));
    assert!(done(&log).is_none());
}

#[tokio::test(start_paused = true)]
async fn no_more_than_parallel_at_once() {
    let goes = (1..=6).map(|id| (id, Goes::Hangs)).collect::<Vec<_>>();
    let devices = Devices::new(&goes);
    let (log, _, handle) = start(plan((1..=6).map(|id| device(id, false)).collect()), devices);
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(states(&log).len(), super::PARALLEL);
    log.cancel();
    let _ = handle.await;
}

#[tokio::test(start_paused = true)]
async fn related_devices_take_turns() {
    // 1 and 2 are behind the switch 3, 4 is elsewhere
    let goes = [(1, Goes::Hangs), (2, Goes::Hangs), (4, Goes::Hangs)];
    let devices = Devices::new(&goes);
    let mut switch = device(3, false);
    switch.related = vec![1, 2];
    let (log, _, handle) = start(
        plan(vec![
            device(1, false),
            device(2, false),
            switch,
            device(4, false),
        ]),
        devices.clone(),
    );
    tokio::time::sleep(Duration::from_secs(10)).await;
    // the switch waits while what is behind it updates, the one elsewhere goes past it
    assert_eq!(
        states(&log),
        [
            (1, State::Started),
            (2, State::Started),
            (4, State::Started)
        ]
    );
    log.cancel();
    let _ = handle.await;
}

#[tokio::test(start_paused = true)]
async fn a_device_waits_for_the_one_in_front() {
    let devices = Devices::new(&[]);
    let mut switch = device(3, false);
    switch.related = vec![1];
    let (log, _, handle) = start(plan(vec![switch, device(1, false)]), devices);
    handle.await.unwrap().unwrap();
    assert_eq!(
        states(&log),
        [
            (3, State::Started),
            (3, State::Done),
            (1, State::Started),
            (1, State::Done)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn each_new_state_of_an_update_is_told() {
    let devices = Devices::new(&[(1, Goes::Hangs)]);
    let phase = devices.phase.clone();
    let (log, _, handle) = start(plan(vec![device(1, false)]), devices);
    let phases = |log: &Log| {
        log.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::RolloutDevice(told) => Some(told.phase),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    tokio::time::sleep(Duration::from_secs(5)).await;
    *phase.lock().unwrap() = Some(UpdateState::Uploading);
    tokio::time::sleep(Duration::from_secs(5)).await;
    *phase.lock().unwrap() = Some(UpdateState::Rebooting);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(
        phases(&log),
        [
            None,
            Some(UpdateState::Uploading),
            Some(UpdateState::Rebooting)
        ]
    );
    log.cancel();
    let _ = handle.await;
}
