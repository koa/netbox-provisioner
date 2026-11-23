//! Scope: Tests of `auto_rollout::changes`; nothing else.
use super::*;
use std::time::Duration;

const DEVICE: DeviceId = DeviceId(1);

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

fn state(change: u64, devices: &Devices) -> (RolloutState, Box<[HoldReason]>) {
    state_of(at(change), DEVICE, devices, true)
}

/// Before any round after it, a change is pending; a round that began before it doesn't count.
#[test]
fn a_change_is_pending_until_a_round_after_it() {
    let mut devices = Devices::default();
    assert_eq!(state(10, &devices).0, RolloutState::Pending);
    devices.note(DEVICE, at(5), Observed::Applied);
    assert_eq!(state(10, &devices).0, RolloutState::Pending);
    devices.note(DEVICE, at(15), Observed::Applied);
    assert_eq!(state(10, &devices).0, RolloutState::RolledOut);
}

/// Once the device was in line after the change, it has it, whatever later rounds saw.
#[test]
fn rolled_out_stays_rolled_out() {
    let mut devices = Devices::default();
    devices.note(DEVICE, at(15), Observed::InLine);
    devices.note(DEVICE, at(20), Observed::Failed);
    assert_eq!(state(10, &devices).0, RolloutState::RolledOut);
    // a newer change is what failed
    assert_eq!(state(18, &devices).0, RolloutState::Failed);
}

/// The newest round after the change says why it isn't there yet.
#[test]
fn the_newest_round_says_why_not() {
    let mut devices = Devices::default();
    devices.note(
        DEVICE,
        at(15),
        Observed::Held([HoldReason::RadioLink].into()),
    );
    assert_eq!(
        state(10, &devices),
        (RolloutState::Waiting, [HoldReason::RadioLink].into())
    );
    devices.note(DEVICE, at(20), Observed::Deferred);
    assert_eq!(state(10, &devices).0, RolloutState::Deferred);
    devices.note(DEVICE, at(25), Observed::Waiting);
    assert_eq!(state(10, &devices).0, RolloutState::Waiting);
}

/// A manual device gets it when an admin applies it.
#[test]
fn a_manual_device_is_applied_by_hand() {
    let mut devices = Devices::default();
    assert_eq!(
        state_of(at(10), DEVICE, &devices, false).0,
        RolloutState::Manual
    );
    devices.note(DEVICE, at(15), Observed::InLine);
    assert_eq!(
        state_of(at(10), DEVICE, &devices, false).0,
        RolloutState::RolledOut
    );
}

/// Only the newest changes are kept.
#[test]
fn the_newest_changes_are_kept() {
    let mut changes = Vec::new();
    for id in 0..(KEPT as u32 + 5) {
        keep(
            &mut changes,
            Change {
                id,
                job: id,
                at: at(id.into()),
                by: "admin".into(),
                owner: None,
                devices: Box::new([]),
            },
        );
    }
    assert_eq!(changes.len(), KEPT);
    assert_eq!(changes[0].id, 5);
}

fn seen(device: u32, automatic: bool, diff: Diff) -> decide::Seen {
    decide::Seen {
        device: DeviceId(device),
        site: automatic.then_some(crate::topology::SiteId(1)),
        diff,
        partners: Box::new([]),
        hold: Box::new([]),
    }
}

fn changed() -> Diff {
    Diff::Changes("script".into())
}

/// A round applying its members notes the rest; the members wait for the batch's outcome.
#[test]
fn a_round_notes_all_but_its_members() {
    let seen = [
        seen(1, true, changed()),
        seen(2, true, Diff::InLine),
        seen(3, true, Diff::Unreached),
        seen(4, false, changed()),
        seen(5, false, Diff::InLine),
    ];
    let decision = Decision {
        members: [DeviceId(1)].into(),
        deferred: [DeviceId(3)].into(),
        ..Decision::default()
    };
    let observed = observed(&seen, &decision);
    assert_eq!(
        observed,
        [
            (DeviceId(2), Observed::InLine),
            (DeviceId(3), Observed::Deferred),
            (DeviceId(5), Observed::InLine),
        ]
        .into()
    );
}

/// Held back, every automatic device with changes waits; the one held names its reasons.
#[test]
fn a_round_held_back_has_its_devices_wait() {
    let seen = [seen(1, true, changed()), seen(2, true, changed())];
    let decision = Decision {
        held: [(DeviceId(2), [HoldReason::PoeCut].into())].into(),
        ..Decision::default()
    };
    let observed = observed(&seen, &decision);
    assert_eq!(
        observed,
        [
            (DeviceId(1), Observed::Waiting),
            (DeviceId(2), Observed::Held([HoldReason::PoeCut].into())),
        ]
        .into()
    );
}
