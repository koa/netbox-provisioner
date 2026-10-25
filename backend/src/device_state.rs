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
    /// the states that may follow any other, whatever the job ended: undoing what it did to
    /// other devices
    const FROM_ANY: &'static [Self] = &[];
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

/// An update or a downgrade of RouterOS (`device::update`); a downgrade has no fallback and no
/// firmware, and goes back to the packages' version in one step.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpdateState {
    /// connecting, reading the device and the version to go to
    Reading,
    /// the watchdogs of the devices that would miss this one, suspended on those
    SuspendingWatchdogs,
    /// waiting until no device powering this one reboots (`PowerChain::updating`)
    WaitingForChain,
    /// the OSPF neighbors up now, to be back after the update
    RememberingNeighbors,
    /// the partition from before copied, its scheduler entry set (`update_fallback::arm`)
    ArmingFallback,
    /// downloading a step's packages, all before the first upload
    Downloading,
    /// uploading them; from here they install at the next boot
    Uploading,
    /// waiting until no device this one powers updates (`PowerChain::rebooting`)
    WaitingToReboot,
    /// packages not wanted uninstalled, rebooting, waiting for the version expected
    Rebooting,
    /// failed with the fallback set: back on the old partition, or its entry taken away
    NotUpdated,
    /// all steps through: the fallback's entry goes
    StoppingFallback,
    /// until the OSPF neighbors from before are back
    WaitingForNeighbors,
    /// the RouterBOARD firmware upgrade set
    Firmware,
    /// waiting until no device this one powers updates
    WaitingToRebootFirmware,
    /// rebooting into the new firmware
    RebootingFirmware,
    /// the watchdogs suspended on the others set again
    RestoringWatchdogs,
}

impl States for UpdateState {
    const FIRST: &'static [Self] = &[UpdateState::Reading];
    const FROM_ANY: &'static [Self] = &[UpdateState::RestoringWatchdogs];
    fn next(self) -> &'static [Self] {
        use UpdateState::*;
        match self {
            Reading => &[SuspendingWatchdogs, WaitingForChain],
            SuspendingWatchdogs => &[WaitingForChain],
            // RouterOS and the packages in line: only the firmware, if at all
            WaitingForChain => &[RememberingNeighbors, Firmware],
            // a downgrade sets no fallback
            RememberingNeighbors => &[ArmingFallback, Downloading],
            ArmingFallback => &[Downloading, NotUpdated],
            Downloading => &[Uploading, NotUpdated],
            Uploading => &[WaitingToReboot, NotUpdated],
            WaitingToReboot => &[Rebooting, NotUpdated],
            // the next step (RouterOS 6 to 7), or through; without a fallback straight on
            Rebooting => &[Downloading, StoppingFallback, WaitingForNeighbors, NotUpdated],
            NotUpdated => &[],
            StoppingFallback => &[WaitingForNeighbors],
            WaitingForNeighbors => &[Firmware],
            Firmware => &[WaitingToRebootFirmware],
            WaitingToRebootFirmware => &[RebootingFirmware],
            RebootingFirmware => &[],
            RestoringWatchdogs => &[],
        }
    }
    fn cancel(self) -> Cancel {
        use UpdateState::*;
        match self {
            Reading | WaitingForChain | RememberingNeighbors | Downloading | Uploading | WaitingToReboot
            | WaitingForNeighbors | WaitingToRebootFirmware => Cancel::Ends,
            _ => Cancel::AfterState,
        }
    }
    fn restart(self) -> Restart {
        use UpdateState::*;
        match self {
            Rebooting | NotUpdated | StoppingFallback | WaitingForNeighbors | Firmware
            | WaitingToRebootFirmware | RebootingFirmware | RestoringWatchdogs => Restart::Continue,
            _ => Restart::Interrupted,
        }
    }
}

/// The state of a job, of whichever kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JobPhase {
    Apply(ApplyState),
    Update(UpdateState),
}

impl From<ApplyState> for JobPhase {
    fn from(state: ApplyState) -> Self {
        JobPhase::Apply(state)
    }
}

impl From<UpdateState> for JobPhase {
    fn from(state: UpdateState) -> Self {
        JobPhase::Update(state)
    }
}

impl JobPhase {
    /// Whether a job in `before` (`None`: just started) may enter `self`.
    pub fn may_follow(self, before: Option<JobPhase>) -> bool {
        fn check<S: States>(before: Option<S>, after: S) -> bool {
            match before {
                None => S::FIRST.contains(&after),
                Some(before) => before.next().contains(&after) || S::FROM_ANY.contains(&after),
            }
        }
        match (before, self) {
            (None, JobPhase::Apply(after)) => check(None, after),
            (None, JobPhase::Update(after)) => check(None, after),
            (Some(JobPhase::Apply(before)), JobPhase::Apply(after)) => check(Some(before), after),
            (Some(JobPhase::Update(before)), JobPhase::Update(after)) => check(Some(before), after),
            // another kind's state
            (Some(_), _) => false,
        }
    }
    pub fn cancel(self) -> Cancel {
        match self {
            JobPhase::Apply(state) => state.cancel(),
            JobPhase::Update(state) => state.cancel(),
        }
    }
    pub fn restart(self) -> Restart {
        match self {
            JobPhase::Apply(state) => state.restart(),
            JobPhase::Update(state) => state.restart(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplyState, JobPhase, States, UpdateState};
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

    fn path<S: Copy + Into<JobPhase>>(states: &[S]) -> bool {
        let mut before = None;
        states.iter().all(|&state| {
            let phase: JobPhase = state.into();
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

    #[test]
    fn update_states_are_reachable() {
        use UpdateState::*;
        all_reachable(&[
            Reading,
            SuspendingWatchdogs,
            WaitingForChain,
            RememberingNeighbors,
            ArmingFallback,
            Downloading,
            Uploading,
            WaitingToReboot,
            Rebooting,
            NotUpdated,
            StoppingFallback,
            WaitingForNeighbors,
            Firmware,
            WaitingToRebootFirmware,
            RebootingFirmware,
        ]);
    }

    #[test]
    fn update_paths() {
        use UpdateState::*;
        // RouterOS 6 to 7 with the firmware, watchdogs suspended
        assert!(path(&[
            Reading, SuspendingWatchdogs, WaitingForChain, RememberingNeighbors, ArmingFallback,
            Downloading, Uploading, WaitingToReboot, Rebooting, Downloading, Uploading, WaitingToReboot,
            Rebooting, StoppingFallback, WaitingForNeighbors, Firmware, WaitingToRebootFirmware,
            RebootingFirmware, RestoringWatchdogs,
        ]));
        // a downgrade
        assert!(path(&[Reading, WaitingForChain, RememberingNeighbors, Downloading, Uploading, WaitingToReboot, Rebooting, WaitingForNeighbors]));
        // rolled back, the watchdogs set again
        assert!(path(&[Reading, WaitingForChain, RememberingNeighbors, ArmingFallback, Downloading, Uploading, WaitingToReboot, Rebooting, NotUpdated, RestoringWatchdogs]));
        // only the firmware
        assert!(path(&[Reading, WaitingForChain, Firmware, WaitingToRebootFirmware, RebootingFirmware]));
        // no reboot without the packages up
        assert!(!path(&[Reading, WaitingForChain, RememberingNeighbors, ArmingFallback, Downloading, Rebooting]));
        // an apply's state in an update
        assert!(!JobPhase::from(ApplyState::Planning).may_follow(Some(UpdateState::Reading.into())));
    }
}
