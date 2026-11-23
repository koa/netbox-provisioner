//! Scope: Tests of `batch::run` with stand-ins for the devices; nothing else.
//!
//! The rules of a batch, with stand-ins for the devices.
use crate::device::batch::{
    Applying, Bystander, PendingSeen, Planning, Reading, Running, Timing, Watched, run, watch,
};
use crate::device::internet::InternetReached;
use crate::device::ospf_auth::{Decision, MemberRead};
use crate::{
    Error,
    device::ros::{apply::FallbackLeftReason, fallback::Pending},
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
    /// where its fallback stands at the first check that reaches it
    pending: Option<Pending>,
    /// it doesn't reach the internet before the batch
    offline_before: bool,
    /// how many looks after its script find it without the internet (OSPF converging)
    offline_looks: usize,
    /// it never reaches the internet after its script
    offline_after: bool,
    /// a neighbor of a segment whose OSPF credentials its plan moved isn't back after its
    /// script
    ospf_missing: bool,
    looks: Arc<Mutex<usize>>,
    seen: Arc<Mutex<Option<Instant>>>,
    calls: Arc<Mutex<Vec<String>>>,
    delays: Arc<Mutex<Vec<Duration>>>,
}

impl Fake {
    fn call(&self, what: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{what} {}", self.name));
    }
    fn fails(&self, fails: bool) -> Result<(), Error> {
        if fails {
            Err(Error::MissingCredentials)
        } else {
            Ok(())
        }
    }
}

impl Planning for Fake {
    type Read = Fake;
    async fn read(self, _log: &Log) -> Result<Fake, Error> {
        self.call("plan");
        self.fails(self.plan_fails)?;
        Ok(self)
    }
}

impl Reading for Fake {
    type Planned = Fake;
    fn ospf(&self) -> Option<&MemberRead> {
        None
    }
    fn plan(self, _ospf: Decision, _log: &Log) -> Result<Option<Fake>, Error> {
        Ok((!self.nothing_to_do).then_some(self))
    }
}

impl Applying for Fake {
    fn radio_wait(&self) -> Option<Duration> {
        self.radio.then_some(Duration::from_millis(200))
    }
    async fn internet_before(&self) -> InternetReached {
        self.call("internet");
        InternetReached {
            v4: true,
            v6: !self.offline_before,
        }
    }
    async fn hold(&mut self, _log: &Log) -> Result<(), Error> {
        if self.radio {
            self.call("hold");
        }
        Ok(())
    }
    async fn arm(
        &self,
        delay: Duration,
        _window: Duration,
        _radio_wait: Option<Duration>,
        _log: &Log,
    ) -> Result<bool, Error> {
        self.delays.lock().unwrap().push(delay);
        self.call("arm");
        self.fails(self.arm_fails)?;
        Ok(!self.no_scheduler)
    }
    async fn write(
        &self,
        _lead: Duration,
        _rearm: Option<Duration>,
        _log: &Log,
    ) -> Result<(), Error> {
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
}

impl Watched for Fake {
    async fn release(&self, back: bool, _log: &Log) {
        if self.radio {
            self.call(if back { "release" } else { "keep" });
        }
    }
    async fn check(&self, _settled: Instant) -> Option<Result<Box<str>, Error>> {
        // a script not started leaves its device as it was
        (!self.lost).then(|| {
            Ok(if self.left || self.start_fails {
                "set x".into()
            } else {
                "".into()
            })
        })
    }
    fn pending(&self) -> Option<PendingSeen> {
        // seen at the first check: a device's fallback fires at one time, however often asked
        let seen = *self.seen.lock().unwrap().get_or_insert_with(Instant::now);
        self.pending
            .map(|pending| PendingSeen { pending, at: seen })
    }
    async fn last_look(&self) -> Result<Box<str>, Error> {
        if self.lost {
            Err(Error::MissingCredentials)
        } else {
            Ok("".into())
        }
    }
    async fn ospf_back(&self) -> Option<bool> {
        Some(!self.ospf_missing)
    }
    async fn internet_after(&self) -> Option<InternetReached> {
        let mut looks = self.looks.lock().unwrap();
        *looks += 1;
        let online = !self.offline_after && *looks > self.offline_looks;
        Some(InternetReached {
            v4: online,
            v6: online,
        })
    }
    async fn stop(&self, _fires: Instant, _delay: Duration, _log: &Log) -> Result<(), Error> {
        self.call("stop");
        Ok(())
    }
    async fn after(&self, _log: &Log) -> Result<(), Error> {
        Ok(())
    }
}

/// A device outside the batch: whether it answers before the scripts and after them.
struct Outside {
    name: &'static str,
    before: bool,
    after: bool,
    looks: Mutex<usize>,
}

impl Outside {
    fn new(name: &'static str, before: bool, after: bool) -> Self {
        Outside {
            name,
            before,
            after,
            looks: Mutex::new(0),
        }
    }
}

impl Bystander for Outside {
    fn name(&self) -> &str {
        self.name
    }
    async fn answers(&self) -> bool {
        let mut looks = self.looks.lock().unwrap();
        *looks += 1;
        if *looks == 1 { self.before } else { self.after }
    }
}

/// What a batch of stand-ins did: the calls in their order, each member's result and log.
struct Outcome {
    calls: Vec<String>,
    results: Vec<Result<(), Error>>,
    logs: Vec<Log>,
}

/// `near` and `far`: the calls and the outcomes.
async fn batch(near: Fake, far: Fake) -> Outcome {
    batch_needing(near, far, false).await
}

/// [`batch`], every device needing the internet before and after (the automatic rollout) with
/// `needs_internet`.
async fn batch_needing(near: Fake, far: Fake, needs_internet: bool) -> Outcome {
    batch_with(near, far, needs_internet, Vec::new()).await
}

/// [`batch_needing`] with `bystanders` outside the batch.
async fn batch_with(
    near: Fake,
    far: Fake,
    needs_internet: bool,
    bystanders: Vec<Outside>,
) -> Outcome {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let delays = Arc::new(Mutex::new(Vec::new()));
    let near = Fake {
        name: "near",
        calls: calls.clone(),
        delays: delays.clone(),
        ..near
    };
    let far = Fake {
        name: "far",
        calls: calls.clone(),
        delays: delays.clone(),
        ..far
    };
    let logs = vec![Log::new("near"), Log::new("far")];
    let results = run(
        vec![near, far],
        vec![crate::topology::DeviceId(1), crate::topology::DeviceId(2)],
        vec!["near".into(), "far".into()],
        logs.clone(),
        Timing {
            delay: Duration::from_millis(300),
            window: Duration::from_millis(100),
            lead: Duration::from_millis(5),
            start: Duration::from_millis(100),
            poll: Duration::from_millis(10),
            settle: Duration::ZERO,
            margin: Duration::ZERO,
        },
        needs_internet,
        bystanders,
    )
    .await;
    // every path of the batch keeps to the apply's state machine
    for log in &logs {
        assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    }
    let calls = calls.lock().unwrap().clone();
    Outcome {
        calls,
        results,
        logs,
    }
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
    let Outcome { calls, results, .. } = batch(Fake::default(), Fake::default()).await;
    assert_eq!(
        calls,
        [
            "plan near",
            "plan far",
            "arm near",
            "write near",
            "arm far",
            "write far",
            "start near",
            "start far",
            "stop near",
            "stop far"
        ]
    );
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

/// A device whose device mode has the scheduler off has no fallback to take away or leave.
#[tokio::test]
async fn a_device_without_scheduler_has_no_fallback_to_stop() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            no_scheduler: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert!(calls.contains(&"stop far".to_string()), "{calls:?}");
    assert!(!calls.contains(&"stop near".to_string()), "{calls:?}");
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let Outcome { logs, .. } = batch(
        Fake {
            no_scheduler: true,
            ..Fake::default()
        },
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert_eq!(left(&logs[0]), []);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
}

#[tokio::test]
async fn one_not_back_leaves_every_fallback() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch(
        Fake::default(),
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "far")),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
}

/// lab-w60-b: reached over another way, but its script never ran
#[tokio::test]
async fn one_back_but_not_in_line_leaves_every_fallback() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch(
        Fake::default(),
        Fake {
            left: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(_)))),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NotApplied]);
}

/// the others' scripts go on; the one not started leaves its device not in line
#[tokio::test]
async fn a_script_not_started_sends_all_back() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch(
        Fake {
            start_fails: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert!(calls.contains(&"start far".to_string()), "{calls:?}");
    assert!(
        !calls
            .iter()
            .any(|call| call.starts_with("stop") || call.starts_with("remove")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "near")),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::NotApplied]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::BatchIncomplete]);
    let started_with_error = logs[0]
        .events()
        .into_iter()
        .any(|event| matches!(event, Event::ApplyStarted(started) if started.error.is_some()));
    assert!(started_with_error);
}

/// a radio changed on one end keeps the other end off as long: both wait for it
#[tokio::test]
async fn a_radio_change_lengthens_every_fallback() {
    let delays = Arc::new(Mutex::new(Vec::new()));
    let near = Fake {
        name: "near",
        radio: true,
        delays: delays.clone(),
        ..Fake::default()
    };
    let far = Fake {
        name: "far",
        delays: delays.clone(),
        ..Fake::default()
    };
    let results = run(
        vec![near, far],
        vec![crate::topology::DeviceId(1), crate::topology::DeviceId(2)],
        vec!["near".into(), "far".into()],
        vec![Log::new("near"), Log::new("far")],
        Timing {
            delay: Duration::from_millis(300),
            window: Duration::from_millis(100),
            lead: Duration::from_millis(5),
            start: Duration::from_millis(100),
            poll: Duration::from_millis(10),
            settle: Duration::ZERO,
            margin: Duration::ZERO,
        },
        false,
        Vec::<Outside>::new(),
    )
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let delays = delays.lock().unwrap();
    assert_eq!(delays.len(), 2);
    assert!(
        delays.iter().all(|d| *d > Duration::from_millis(400)),
        "{delays:?}"
    );
}

/// A changed radio holds the watchdogs before the fallbacks, restores them once all are
/// back or nothing is applied, and leaves them while the fallbacks may still fire.
#[tokio::test]
async fn a_changed_radio_holds_the_watchdogs_until_all_are_back() {
    let radio = || Fake {
        radio: true,
        ..Fake::default()
    };
    let Outcome { calls, results, .. } = batch(radio(), Fake::default()).await;
    assert_eq!(
        calls,
        [
            "plan near",
            "plan far",
            "hold near",
            "arm near",
            "write near",
            "arm far",
            "write far",
            "start near",
            "start far",
            "stop near",
            "stop far",
            "release near"
        ]
    );
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let Outcome { calls, .. } = batch(
        radio(),
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert_eq!(
        calls.last().map(String::as_str),
        Some("keep near"),
        "{calls:?}"
    );
    let Outcome { calls, .. } = batch(
        radio(),
        Fake {
            write_fails: true,
            ..Fake::default()
        },
    )
    .await;
    assert_eq!(
        calls.last().map(String::as_str),
        Some("release near"),
        "{calls:?}"
    );
}

#[tokio::test]
async fn a_failing_write_starts_nothing() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            write_fails: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert_eq!(
        calls,
        [
            "plan near",
            "plan far",
            "arm near",
            "write near",
            "arm far",
            "write far",
            "remove near",
            "stop near",
            "remove far",
            "stop far"
        ]
    );
    assert!(
        matches!(&results[0], Err(Error::MissingCredentials)),
        "{results:?}"
    );
    assert!(
        matches!(&results[1], Err(Error::BatchStopped(d)) if &**d == "near"),
        "{results:?}"
    );
}

#[tokio::test]
async fn a_failing_plan_applies_nothing() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            plan_fails: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert_eq!(calls, ["plan near", "plan far"]);
    assert!(
        matches!(&results[0], Err(Error::MissingCredentials)),
        "{results:?}"
    );
    assert!(
        matches!(&results[1], Err(Error::BatchStopped(d)) if &**d == "near"),
        "{results:?}"
    );
}

#[tokio::test]
async fn each_failing_plan_keeps_its_own_error() {
    let failing = || Fake {
        plan_fails: true,
        ..Fake::default()
    };
    let Outcome { results, .. } = batch(failing(), failing()).await;
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::MissingCredentials))),
        "{results:?}"
    );
}

#[tokio::test]
async fn a_failing_fallback_takes_the_others_away() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            arm_fails: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert_eq!(
        calls,
        [
            "plan near",
            "plan far",
            "arm near",
            "arm far",
            "write far",
            "remove near",
            "remove far",
            "stop far"
        ]
    );
    assert!(
        matches!(&results[1], Err(Error::BatchStopped(_))),
        "{results:?}"
    );
}

#[tokio::test]
async fn a_device_in_line_is_left_out() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            nothing_to_do: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert_eq!(
        calls,
        [
            "plan near",
            "plan far",
            "arm far",
            "write far",
            "start far",
            "stop far"
        ]
    );
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

/// A device whose fallback fired takes the batch back at once, without waiting for one still
/// not reached until the fallbacks' delay.
#[tokio::test]
async fn a_fired_fallback_ends_the_wait() {
    let started = Instant::now();
    let near = Fake {
        pending: Some(Pending::Gone),
        left: true,
        ..Fake::default()
    };
    let Outcome {
        calls,
        results,
        logs,
    } = batch(
        near,
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "{:?}",
        started.elapsed()
    );
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(results.iter().all(Result::is_err));
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
}

/// The earliest fallback a reached device reports bounds the wait for the others: its script
/// brought it forward.
#[tokio::test]
async fn a_fallback_brought_forward_bounds_the_wait() {
    let started = Instant::now();
    let near = Fake {
        pending: Some(Pending::Armed(Some(Duration::from_millis(50)))),
        ..Fake::default()
    };
    let Outcome { calls, results, .. } = batch(
        near,
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "{:?}",
        started.elapsed()
    );
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(results.iter().all(Result::is_err));
}

/// A batch going on after a restart, its scripts running: `near` and `far`, the calls and the
/// outcomes.
async fn resumed(near: Fake, far: Fake) -> Outcome {
    use crate::device_state::ApplyState;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let logs = vec![Log::new("near"), Log::new("far")];
    let running = [
        Fake {
            name: "near",
            calls: calls.clone(),
            ..near
        },
        Fake {
            name: "far",
            calls: calls.clone(),
            ..far
        },
    ]
    .into_iter()
    .zip(&logs)
    .enumerate()
    .map(|(index, (member, log))| {
        log.resume_in(ApplyState::ScriptRunning);
        Running {
            index,
            name: member.name.into(),
            log: log.clone(),
            fallback: !member.no_scheduler,
            member,
            checked: None,
            internet: None,
            ospf: None,
        }
    })
    .collect();
    let timing = Timing {
        delay: Duration::from_millis(300),
        window: Duration::from_millis(100),
        lead: Duration::from_millis(5),
        start: Duration::from_millis(100),
        poll: Duration::from_millis(10),
        settle: Duration::ZERO,
        margin: Duration::ZERO,
    };
    let fires = Instant::now() + Duration::from_millis(300);
    let results = watch(
        running,
        &logs,
        &timing,
        fires,
        timing.delay,
        vec![None, None],
        false,
        &[] as &[&Outside],
    )
    .await;
    for log in &logs {
        assert!(log.unexpected().is_empty(), "{:?}", log.unexpected());
    }
    let calls = calls.lock().unwrap().clone();
    Outcome {
        calls,
        results,
        logs,
    }
}

/// After a restart the batch goes on where its scripts run: all back, every fallback goes;
/// one not back, none does.
#[tokio::test]
async fn a_resumed_batch_keeps_its_rules() {
    let Outcome { calls, results, .. } = resumed(Fake::default(), Fake::default()).await;
    assert_eq!(calls, ["stop near", "stop far"]);
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let Outcome {
        calls,
        results,
        logs,
    } = resumed(
        Fake::default(),
        Fake {
            lost: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(calls.is_empty(), "{calls:?}");
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(_)))),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NotReached]);
}

/// Needing the internet (the automatic rollout), a device without it over IPv6 stops the
/// batch before anything is armed.
#[tokio::test]
async fn no_internet_before_applies_nothing() {
    let Outcome { calls, results, .. } = batch_needing(
        Fake::default(),
        Fake {
            offline_before: true,
            ..Fake::default()
        },
        true,
    )
    .await;
    assert_eq!(
        calls,
        ["plan near", "plan far", "internet near", "internet far"]
    );
    assert!(matches!(&results[1], Err(Error::NoInternet(d)) if &**d == "far"));
    assert!(matches!(&results[0], Err(Error::BatchStopped(d)) if &**d == "far"));
}

/// The internet coming back a few looks after the scripts (OSPF converging) is waited for;
/// then the fallbacks go.
#[tokio::test]
async fn the_internet_is_waited_for_after_the_scripts() {
    let Outcome { calls, results, .. } = batch_needing(
        Fake {
            offline_looks: 3,
            ..Fake::default()
        },
        Fake::default(),
        true,
    )
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert!(calls.iter().any(|call| call == "stop near"), "{calls:?}");
}

/// A device in line but without the internet after its script sends all back.
#[tokio::test]
async fn no_internet_after_sends_all_back() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch_needing(
        Fake::default(),
        Fake {
            offline_after: true,
            ..Fake::default()
        },
        true,
    )
    .await;
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "far")),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::NoInternet]);
}

/// A device in line whose moved OSPF segment has no neighbor back sends all back, to the
/// credentials before; also in a batch by hand.
#[tokio::test]
async fn an_ospf_neighbor_missing_after_sends_all_back() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch(
        Fake::default(),
        Fake {
            ospf_missing: true,
            ..Fake::default()
        },
    )
    .await;
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "far")),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert_eq!(left(&logs[1]), [FallbackLeftReason::OspfMissing]);
}

/// Without the need, nothing pings the internet.
#[tokio::test]
async fn a_batch_by_hand_doesnt_ask_for_the_internet() {
    let Outcome { calls, results, .. } = batch(
        Fake {
            offline_before: true,
            offline_after: true,
            ..Fake::default()
        },
        Fake::default(),
    )
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert!(
        !calls.iter().any(|call| call.starts_with("internet")),
        "{calls:?}"
    );
}

/// A device outside the batch answering before and after: the fallbacks go.
#[tokio::test]
async fn a_device_outside_answering_again_lets_the_fallbacks_go() {
    let Outcome { calls, results, .. } = batch_with(
        Fake::default(),
        Fake::default(),
        false,
        vec![Outside::new("behind", true, true)],
    )
    .await;
    assert_eq!(calls.iter().filter(|c| c.starts_with("stop")).count(), 2);
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

/// A device outside the batch cut off by it sends all back, though every member is in line.
#[tokio::test]
async fn a_device_outside_cut_off_sends_all_back() {
    let Outcome {
        calls,
        results,
        logs,
    } = batch_with(
        Fake::default(),
        Fake::default(),
        true,
        vec![Outside::new("behind", true, false)],
    )
    .await;
    assert!(
        !calls.iter().any(|call| call.starts_with("stop")),
        "{calls:?}"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(Error::BatchIncomplete(d)) if &**d == "behind")),
        "{results:?}"
    );
    assert_eq!(left(&logs[0]), [FallbackLeftReason::BatchIncomplete]);
    assert!(logs[0].events().iter().any(|event| matches!(
        event,
        Event::BatchIncomplete(incomplete) if *incomplete.cut_off == ["behind".into()]
    )));
}

/// A device outside not answering before the batch isn't waited for.
#[tokio::test]
async fn a_device_outside_away_before_is_left_out() {
    let Outcome { results, logs, .. } = batch_with(
        Fake::default(),
        Fake::default(),
        false,
        vec![Outside::new("away", false, false)],
    )
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert!(logs[0].events().iter().any(|event| matches!(
        event,
        Event::BystandersWatched(watched)
            if watched.watched.is_empty() && *watched.not_answering == ["away".into()]
    )));
}
