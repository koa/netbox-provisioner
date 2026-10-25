//! The states of a job, one state machine per kind (`docs/jobs.md`). A job
//! reports each state it enters ([`crate::job::Log::enter`]); the tables here say which may
//! follow which, whether cancelling ends a state right away, and what a restart of the server
//! does to a job in it. Pure: nothing here talks to a device.
//!
//! Ending is no state: a job may end in any of them (done, failed, cancelled), and the state
//! it ended in stays with it.
use async_graphql::{Enum, SimpleObject, Union};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// What cancelling does in a state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Cancel {
    /// the job ends at once (taking away what the state set up, as the state says)
    Ends,
    /// the state runs to its end first: what it started has to finish
    AfterState,
}

/// What becomes of a job in a state when the server restarts. Only where ending would take a
/// change that went well back (a fallback firing) is it saved and continued; everything else
/// waits or reads and may as well end (`docs/jobs.md`, „Weiterführen nach einem Neustart“).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Restart {
    /// saved on entering the state, continued after a restart
    Continue,
    /// the job ends as interrupted; what it left on the device stays
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum, Serialize, Deserialize)]
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
            ScriptRunning | StoppingFallback => Restart::Continue,
            _ => Restart::Interrupted,
        }
    }
}

/// An update or a downgrade of RouterOS (`device::update`); a downgrade has no fallback and no
/// firmware, and goes back to the packages' version in one step.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum, Serialize, Deserialize)]
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
            // with the fallback set; without one Rebooting ends harmlessly as well
            Rebooting | NotUpdated | StoppingFallback => Restart::Continue,
            _ => Restart::Interrupted,
        }
    }

}

/// A reboot, a repartition or a boot from another partition (`update::reboot`, `partitions`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum RebootState {
    /// connecting, reading what the reboot needs (the partitions)
    Reading,
    /// waiting until no device this one powers updates (`PowerChain::rebooting`)
    WaitingForChain,
    /// the command sent (reboot, repartition, activate), waiting for the device to be back
    Rebooting,
}

impl States for RebootState {
    const FIRST: &'static [Self] = &[RebootState::Reading];
    fn next(self) -> &'static [Self] {
        use RebootState::*;
        match self {
            Reading => &[WaitingForChain],
            WaitingForChain => &[Rebooting],
            Rebooting => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            RebootState::Rebooting => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// A supplier cutting the power of a device it feeds (`power_cycle`): a job of the supplier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum PowerCycleState {
    /// connecting to the supplier
    Reading,
    /// waiting until neither the fed device nor one it powers updates (`PowerChain::power_cut`)
    WaitingForChain,
    /// the port switched off for a few seconds and on again
    Cutting,
    /// until the fed device answers again; it boots meanwhile
    WaitingForDevice,
}

impl States for PowerCycleState {
    const FIRST: &'static [Self] = &[PowerCycleState::Reading];
    fn next(self) -> &'static [Self] {
        use PowerCycleState::*;
        match self {
            Reading => &[WaitingForChain],
            WaitingForChain => &[Cutting],
            Cutting => &[WaitingForDevice],
            WaitingForDevice => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            PowerCycleState::Cutting => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// A change of the device mode (`device_mode::change`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum DeviceModeState {
    /// connecting, reading which features are off
    Reading,
    /// waiting until no device this one powers updates: confirming cuts the power or reboots
    WaitingForChain,
    /// `/system/device-mode/update` sent, waiting for its countdown
    Requesting,
    /// the device counts down until its button is pressed or its power cut; the connection
    /// has to stay, closing it cancels the change
    WaitingForConfirmation,
    /// the device went down, waiting until it runs with the features on
    Rebooting,
}

impl States for DeviceModeState {
    const FIRST: &'static [Self] = &[DeviceModeState::Reading];
    fn next(self) -> &'static [Self] {
        use DeviceModeState::*;
        match self {
            Reading => &[WaitingForChain],
            WaitingForChain => &[Requesting],
            Requesting => &[WaitingForConfirmation],
            WaitingForConfirmation => &[Rebooting],
            Rebooting => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            DeviceModeState::Requesting => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// A setup over mac-telnet from a neighbor (`bootstrap`), which keeps the neighbor busy.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum BootstrapState {
    /// the neighbor allows SSH forwarding and has the port
    CheckingNeighbor,
    /// on the neighbor's console, logging in to the device and setting it up
    Console,
    /// until the device's API answers at its new address
    WaitingForApi,
}

impl States for BootstrapState {
    const FIRST: &'static [Self] = &[BootstrapState::CheckingNeighbor];
    fn next(self) -> &'static [Self] {
        use BootstrapState::*;
        match self {
            CheckingNeighbor => &[Console],
            Console => &[WaitingForApi],
            WaitingForApi => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            BootstrapState::Console => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// Taking away what a job left on the device (`leftovers::clear`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum CleanupState {
    /// the leftover chosen goes
    Clearing,
    /// "Jetzt zurück": waiting until no device this one powers updates
    WaitingForChain,
    /// the backup loads, waiting for the device to be back
    Rebooting,
}

impl States for CleanupState {
    const FIRST: &'static [Self] = &[CleanupState::Clearing];
    fn next(self) -> &'static [Self] {
        match self {
            CleanupState::Clearing => &[CleanupState::WaitingForChain],
            CleanupState::WaitingForChain => &[CleanupState::Rebooting],
            CleanupState::Rebooting => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            CleanupState::Clearing => Cancel::AfterState,
            CleanupState::WaitingForChain | CleanupState::Rebooting => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// Writing a helper's plan to NetBox, or taking such a write back (`helper::write`, `undo`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Enum)]
pub enum NetboxState {
    /// the plan computed again and checked against the one shown
    Planning,
    /// its steps, one after the other; each written one counts, also when a later fails
    Writing,
    /// the written objects set back, the last first
    Undoing,
    /// the topology read again, so the next page shows what NetBox has
    Refetching,
}

impl States for NetboxState {
    const FIRST: &'static [Self] = &[NetboxState::Planning, NetboxState::Undoing];
    const FROM_ANY: &'static [Self] = &[NetboxState::Refetching];
    fn next(self) -> &'static [Self] {
        match self {
            NetboxState::Planning => &[NetboxState::Writing],
            NetboxState::Writing | NetboxState::Undoing | NetboxState::Refetching => &[],
        }
    }
    fn cancel(self) -> Cancel {
        match self {
            NetboxState::Refetching => Cancel::AfterState,
            _ => Cancel::Ends,
        }
    }
    fn restart(self) -> Restart {
        Restart::Interrupted
    }

}

/// [`JobPhase`] over the kinds' state types: one variant each, `From`, the checks handed on to
/// the kind, and for GraphQL [`JobPhaseObject`] with an object per kind.
macro_rules! job_phase {
    ($($variant:ident($state:ty, $object:ident)),* $(,)?) => {
        /// The state of a job, of whichever kind.
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub enum JobPhase {
            $($variant($state),)*
        }

        $(impl From<$state> for JobPhase {
            fn from(state: $state) -> Self {
                JobPhase::$variant(state)
            }
        })*

        $(
            /// The state of a job of this kind.
            #[derive(Debug, Clone, Copy, PartialEq, SimpleObject)]
            pub struct $object {
                pub state: $state,
            }
        )*

        /// [`JobPhase`] for GraphQL: a union needs an object per kind.
        #[derive(Debug, Clone, Copy, PartialEq, Union)]
        pub enum JobPhaseObject {
            $($variant($object),)*
        }

        impl From<JobPhase> for JobPhaseObject {
            fn from(phase: JobPhase) -> Self {
                match phase {
                    $(JobPhase::$variant(state) => JobPhaseObject::$variant($object { state }),)*
                }
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
                    $((None, JobPhase::$variant(after)) => check(None, after),
                    (Some(JobPhase::$variant(before)), JobPhase::$variant(after)) => check(Some(before), after),)*
                    // another kind's state
                    #[allow(unreachable_patterns)]
                    (Some(_), _) => false,
                }
            }
            pub fn cancel(self) -> Cancel {
                match self {
                    $(JobPhase::$variant(state) => state.cancel(),)*
                }
            }
            pub fn restart(self) -> Restart {
                match self {
                    $(JobPhase::$variant(state) => state.restart(),)*
                }
            }
        }
    };
}

job_phase!(
    Apply(ApplyState, ApplyPhase),
    Update(UpdateState, UpdatePhase),
    Reboot(RebootState, RebootPhase),
    PowerCycle(PowerCycleState, PowerCyclePhase),
    DeviceMode(DeviceModeState, DeviceModePhase),
    Bootstrap(BootstrapState, BootstrapPhase),
    Cleanup(CleanupState, CleanupPhase),
    Netbox(NetboxState, NetboxPhase),
);

#[cfg(test)]
mod tests {
    use super::{
        ApplyState, BootstrapState, CleanupState, DeviceModeState, JobPhase, NetboxState, PowerCycleState,
        RebootState, States, UpdateState,
    };
    use std::collections::BTreeSet;

    /// Every state of `all` can be reached from a first one.
    fn all_reachable<S: States + Ord>(all: &[S]) {
        // those following any state follow a first one too
        let mut seen: BTreeSet<S> = S::FIRST.iter().chain(S::FROM_ANY).copied().collect();
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
            RestoringWatchdogs,
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

    #[test]
    fn the_other_kinds() {
        use RebootState::*;
        all_reachable(&[Reading, WaitingForChain, Rebooting]);
        assert!(path(&[Reading, WaitingForChain, Rebooting]));
        assert!(!path(&[Reading, Rebooting]));
        all_reachable(&[
            PowerCycleState::Reading,
            PowerCycleState::WaitingForChain,
            PowerCycleState::Cutting,
            PowerCycleState::WaitingForDevice,
        ]);
        all_reachable(&[
            DeviceModeState::Reading,
            DeviceModeState::WaitingForChain,
            DeviceModeState::Requesting,
            DeviceModeState::WaitingForConfirmation,
            DeviceModeState::Rebooting,
        ]);
        all_reachable(&[BootstrapState::CheckingNeighbor, BootstrapState::Console, BootstrapState::WaitingForApi]);
        all_reachable(&[CleanupState::Clearing, CleanupState::WaitingForChain, CleanupState::Rebooting]);
        all_reachable(&[NetboxState::Planning, NetboxState::Writing, NetboxState::Undoing, NetboxState::Refetching]);
        // the topology read again after a failed write too
        assert!(path(&[NetboxState::Planning, NetboxState::Refetching]));
        assert!(path(&[NetboxState::Undoing, NetboxState::Refetching]));
        assert!(!path(&[NetboxState::Writing]));
    }
}
