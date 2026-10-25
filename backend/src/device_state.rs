//! The states of a job, one state machine per kind (`docs/concepts/device-state.md`). A job
//! reports each state it enters ([`crate::job::Log::enter`]); the tables here say which may
//! follow which, whether cancelling ends a state right away, and what a restart of the server
//! does to a job in it. Pure: nothing here talks to a device.
//!
//! Ending is no state: a job may end in any of them (done, failed, cancelled), and the state
//! it ended in stays with it.
use std::fmt::Debug;

/// What cancelling does in a state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Cancel {
    /// the job ends at once (taking away what the state set up, as the state says)
    Ends,
    /// the state runs to its end first: what it started has to finish
    AfterState,
}

/// What becomes of a job in a state when the server restarts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Restart {
    /// the state only reads and takes away the job's own self-triggers: it may be continued
    Continue,
    /// the job ends as interrupted; what it left on the device stays and is watched
    Interrupted,
}

/// The states of one kind of job.
pub trait States: Copy + Eq + Debug + 'static {
    /// the states a job of the kind starts in
    const FIRST: &'static [Self];
    /// the states that may follow `self`
    fn next(self) -> &'static [Self];
    fn cancel(self) -> Cancel;
    fn restart(self) -> Restart;
}

/// An apply, alone or in a batch (`DeviceCfg::apply`, `device::batch`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApplyState {
    /// reading the device and NetBox, rendering the script
    Planning,
    /// a radio changes: the watchdogs it may set off are suspended (`RadioHold`)
    SuspendingWatchdogs,
    /// the backup and the scheduler entry loading it (`arm_fallback_on`)
    ArmingFallback,
    /// the script goes onto the device (`write_script`)
    WritingScript,
    /// it is started and looked for in `/system/script/job`
    StartingScript,
    /// it runs without the provisioner, which looks at its marks
    ScriptRunning,
    /// before the script ran: its scheduler entry and the fallback go again
    TakingBack,
    /// through and reached: the fallback goes (`disarm_unless_due`)
    StoppingFallback,
    /// not reached, or the fallback due: it stays; alone the apply waits for it to fire and
    /// the device to come back
    NotReached,
    /// the diff, once the device settled
    Checking,
    /// the CAPs' radios provisioned again, the watchdogs set again
    Finishing,
}

impl States for ApplyState {
    const FIRST: &'static [Self] = &[ApplyState::Planning];
    fn next(self) -> &'static [Self] {
        use ApplyState::*;
        match self {
            Planning => &[SuspendingWatchdogs, ArmingFallback],
            SuspendingWatchdogs => &[ArmingFallback],
            // in a batch another device failing takes it back
            ArmingFallback => &[WritingScript, TakingBack],
            WritingScript => &[StartingScript, TakingBack],
            StartingScript => &[ScriptRunning, TakingBack],
            // without a fallback (the device mode locks the scheduler) straight to the diff, in a
            // batch (which diffs while the scripts run) to what follows
            ScriptRunning => &[StoppingFallback, NotReached, Checking, Finishing],
            // alone: not yet taken away, it looks again; in a batch the diffs are done
            StoppingFallback => &[Checking, NotReached, ScriptRunning, Finishing],
            TakingBack | NotReached => &[],
            Checking => &[Finishing],
            Finishing => &[],
        }
    }
    fn cancel(self) -> Cancel {
        use ApplyState::*;
        match self {
            StartingScript | StoppingFallback | Finishing | TakingBack => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        use ApplyState::*;
        match self {
            ScriptRunning | StoppingFallback | NotReached | Checking | Finishing => Restart::Continue,
            _ => Restart::Interrupted,
        }
    }
}

/// The state of a job, of whichever kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JobPhase {
    Apply(ApplyState),
}

impl From<ApplyState> for JobPhase {
    fn from(state: ApplyState) -> Self {
        JobPhase::Apply(state)
    }
}

impl JobPhase {
    /// Whether a job in `before` (`None`: just started) may enter `self`.
    pub fn may_follow(self, before: Option<JobPhase>) -> bool {
        fn check<S: States>(before: Option<S>, after: S) -> bool {
            match before {
                None => S::FIRST.contains(&after),
                Some(before) => before.next().contains(&after),
            }
        }
        match (before, self) {
            (None, JobPhase::Apply(after)) => check(None, after),
            (Some(JobPhase::Apply(before)), JobPhase::Apply(after)) => check(Some(before), after),
        }
    }
    pub fn cancel(self) -> Cancel {
        match self {
            JobPhase::Apply(state) => state.cancel(),
        }
    }
    pub fn restart(self) -> Restart {
        match self {
            JobPhase::Apply(state) => state.restart(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplyState, JobPhase, States};
    use std::collections::BTreeSet;

    /// Every state of `all` can be reached from a first one.
    fn all_reachable<S: States + Ord>(all: &[S]) {
        let mut seen: BTreeSet<S> = S::FIRST.iter().copied().collect();
        let mut todo: Vec<S> = S::FIRST.to_vec();
        while let Some(state) = todo.pop() {
            for &next in state.next() {
                if seen.insert(next) {
                    todo.push(next);
                }
            }
        }
        let missing: Vec<_> = all.iter().filter(|s| !seen.contains(s)).collect();
        assert!(missing.is_empty(), "not reachable: {missing:?}");
    }

    fn path(states: &[ApplyState]) -> bool {
        let mut before = None;
        states.iter().all(|&state| {
            let phase = JobPhase::from(state);
            let ok = phase.may_follow(before);
            before = Some(phase);
            ok
        })
    }

    #[test]
    fn apply_states_are_reachable() {
        use ApplyState::*;
        all_reachable(&[
            Planning,
            SuspendingWatchdogs,
            ArmingFallback,
            WritingScript,
            StartingScript,
            ScriptRunning,
            TakingBack,
            StoppingFallback,
            NotReached,
            Checking,
            Finishing,
        ]);
    }

    #[test]
    fn apply_paths() {
        use ApplyState::*;
        // alone, through
        assert!(path(&[Planning, ArmingFallback, WritingScript, StartingScript, ScriptRunning, StoppingFallback, Checking, Finishing]));
        // with a radio, not reached
        assert!(path(&[Planning, SuspendingWatchdogs, ArmingFallback, WritingScript, StartingScript, ScriptRunning, NotReached]));
        // the fallback not taken away yet: looking again
        assert!(path(&[Planning, ArmingFallback, WritingScript, StartingScript, ScriptRunning, StoppingFallback, ScriptRunning, StoppingFallback, Checking, Finishing]));
        // a batch, through
        assert!(path(&[Planning, ArmingFallback, WritingScript, StartingScript, ScriptRunning, StoppingFallback, Finishing]));
        // no script without a fallback being armed first
        assert!(!path(&[Planning, WritingScript]));
        // started in the middle
        assert!(!path(&[ScriptRunning]));
    }
}
