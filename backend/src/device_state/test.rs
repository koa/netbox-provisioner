//! Scope: Tests of `device_state`; nothing else.
use super::{
    ApplyState, BootstrapState, CleanupState, DeviceModeState, JobPhase, NetboxState,
    PowerCycleState, RebootState, States, UpdateState,
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
        InstallingCertificate,
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
    assert!(path(&[
        Planning,
        ArmingFallback,
        WritingScript,
        StartingScript,
        ScriptRunning,
        StoppingFallback,
        Checking,
        Finishing
    ]));
    // with a radio, not reached
    assert!(path(&[
        Planning,
        SuspendingWatchdogs,
        ArmingFallback,
        WritingScript,
        StartingScript,
        ScriptRunning,
        NotReached
    ]));
    // nothing to apply but the current password (A9)
    assert!(path(&[Planning, Finishing]));
    // WebFig's certificate first (A7)
    assert!(path(&[
        Planning,
        InstallingCertificate,
        Planning,
        ArmingFallback,
        WritingScript,
        StartingScript,
        ScriptRunning,
        StoppingFallback,
        Checking,
        Finishing
    ]));
    // the fallback not taken away yet: looking again
    assert!(path(&[
        Planning,
        ArmingFallback,
        WritingScript,
        StartingScript,
        ScriptRunning,
        StoppingFallback,
        ScriptRunning,
        StoppingFallback,
        Checking,
        Finishing
    ]));
    // a batch, through
    assert!(path(&[
        Planning,
        ArmingFallback,
        WritingScript,
        StartingScript,
        ScriptRunning,
        StoppingFallback,
        Finishing
    ]));
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
        Reading,
        SuspendingWatchdogs,
        WaitingForChain,
        RememberingNeighbors,
        ArmingFallback,
        Downloading,
        Uploading,
        WaitingToReboot,
        Rebooting,
        Downloading,
        Uploading,
        WaitingToReboot,
        Rebooting,
        StoppingFallback,
        WaitingForNeighbors,
        Firmware,
        WaitingToRebootFirmware,
        RebootingFirmware,
        RestoringWatchdogs,
    ]));
    // a downgrade
    assert!(path(&[
        Reading,
        WaitingForChain,
        RememberingNeighbors,
        Downloading,
        Uploading,
        WaitingToReboot,
        Rebooting,
        WaitingForNeighbors
    ]));
    // rolled back, the watchdogs set again
    assert!(path(&[
        Reading,
        WaitingForChain,
        RememberingNeighbors,
        ArmingFallback,
        Downloading,
        Uploading,
        WaitingToReboot,
        Rebooting,
        NotUpdated,
        RestoringWatchdogs
    ]));
    // only the firmware
    assert!(path(&[
        Reading,
        WaitingForChain,
        Firmware,
        WaitingToRebootFirmware,
        RebootingFirmware
    ]));
    // no reboot without the packages up
    assert!(!path(&[
        Reading,
        WaitingForChain,
        RememberingNeighbors,
        ArmingFallback,
        Downloading,
        Rebooting
    ]));
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
    all_reachable(&[
        BootstrapState::SuspendingWatchdogs,
        BootstrapState::CheckingNeighbor,
        BootstrapState::Console,
        BootstrapState::WaitingForApi,
        BootstrapState::RestoringWatchdogs,
    ]);
    all_reachable(&[
        CleanupState::Clearing,
        CleanupState::WaitingForChain,
        CleanupState::Rebooting,
    ]);
    all_reachable(&[
        NetboxState::Planning,
        NetboxState::Writing,
        NetboxState::Undoing,
        NetboxState::Refetching,
    ]);
    // the topology read again after a failed write too
    assert!(path(&[NetboxState::Planning, NetboxState::Refetching]));
    assert!(path(&[NetboxState::Undoing, NetboxState::Refetching]));
    assert!(!path(&[NetboxState::Writing]));
}
