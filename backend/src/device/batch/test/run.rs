//! The rules of a batch, with stand-ins for the devices.
use crate::device::batch::{Applying, Timing, run};
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
