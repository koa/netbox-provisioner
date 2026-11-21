//! Scope: Tests of `job`; nothing else.
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
