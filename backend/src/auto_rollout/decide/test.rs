//! Scope: Tests of `auto_rollout::decide`; nothing else.
use super::*;

const SITE: SiteId = SiteId(1);

fn auto(device: u32, diff: Diff) -> Seen {
    Seen {
        device: DeviceId(device),
        site: Some(SITE),
        diff,
        partners: Box::new([]),
    }
}

fn manual(device: u32, diff: Diff) -> Seen {
    Seen {
        site: None,
        ..auto(device, diff)
    }
}

fn linked(mut seen: Seen, partners: &[u32]) -> Seen {
    seen.partners = partners.iter().copied().map(DeviceId).collect();
    seen
}

fn changes(script: &str) -> Diff {
    Diff::Changes(script.into())
}

fn ids(ids: &[u32]) -> BTreeSet<DeviceId> {
    ids.iter().copied().map(DeviceId).collect()
}

fn plain(seen: &[Seen]) -> Decision {
    decide(seen, &BTreeMap::new(), &BTreeSet::new(), &BTreeSet::new())
}

/// The automatic devices with changes go out together, those in line not.
#[test]
fn every_automatic_device_with_changes_is_applied() {
    let decision = plain(&[
        auto(1, changes("a")),
        auto(2, Diff::InLine),
        auto(3, changes("c")),
    ]);
    assert_eq!(decision.members, ids(&[1, 3]));
    assert!(decision.deferred.is_empty());
}

/// One not reached waits, the others go on without it.
#[test]
fn an_unreached_device_is_passed_over() {
    let decision = plain(&[auto(1, changes("a")), auto(2, Diff::Unreached)]);
    assert_eq!(decision.members, ids(&[1]));
    assert_eq!(decision.deferred, ids(&[2]));
}

/// The other end of a radio link of an unreached device isn't touched, along a chain of links
/// too.
#[test]
fn the_partners_of_an_unreached_device_wait() {
    let decision = plain(&[
        linked(auto(1, Diff::Unreached), &[2]),
        linked(auto(2, changes("b")), &[1, 3]),
        linked(auto(3, changes("c")), &[2]),
        auto(4, changes("d")),
    ]);
    assert_eq!(decision.members, ids(&[4]));
    assert_eq!(decision.deferred, ids(&[1, 2, 3]));
}

/// A manual device next to them whose diff changed holds the whole round back, round after
/// round, until it is applied by hand (in line) or released.
#[test]
fn a_changed_manual_device_holds_the_round_back() {
    let seen = [auto(1, changes("a")), manual(9, changes("m"))];
    let decision = plain(&seen);
    assert!(decision.members.is_empty());
    assert_eq!(decision.held_by, ids(&[9]));
    let again = decide(
        &seen,
        &decision.baselines,
        &BTreeSet::new(),
        &BTreeSet::new(),
    );
    assert_eq!(again.held_by, ids(&[9]));
    let by_hand = decide(
        &[auto(1, changes("a")), manual(9, Diff::InLine)],
        &again.baselines,
        &BTreeSet::new(),
        &BTreeSet::new(),
    );
    assert_eq!(by_hand.members, ids(&[1]));
}

/// A manual device whose diff is the one seen before doesn't hold anything back: it has
/// changes of its own an operator left there.
#[test]
fn a_manual_device_as_before_doesnt_hold_back() {
    let baselines = [(DeviceId(9), "m".into())].into();
    let decision = decide(
        &[auto(1, changes("a")), manual(9, changes("m"))],
        &baselines,
        &BTreeSet::new(),
        &BTreeSet::new(),
    );
    assert_eq!(decision.members, ids(&[1]));
    assert_eq!(decision.baselines, baselines);
}

/// Released by an admin, the manual device is applied with the automatic ones.
#[test]
fn a_released_manual_device_goes_with_them() {
    let decision = decide(
        &[auto(1, changes("a")), manual(9, changes("m"))],
        &BTreeMap::new(),
        &ids(&[9]),
        &BTreeSet::new(),
    );
    assert_eq!(decision.members, ids(&[1, 9]));
}

/// A manual device not reached keeps what was seen of it; one in line drops it.
#[test]
fn the_manual_diffs_are_kept_for_the_next_round() {
    let baselines = [(DeviceId(8), "x".into()), (DeviceId(9), "y".into())].into();
    let decision = decide(
        &[manual(8, Diff::Unreached), manual(9, Diff::InLine)],
        &baselines,
        &BTreeSet::new(),
        &BTreeSet::new(),
    );
    assert_eq!(
        decision.baselines,
        [(DeviceId(8), Fingerprint::from("x"))].into()
    );
}

/// A site switched on with changes on its devices applies none of them.
#[test]
fn a_site_switched_on_must_be_in_line() {
    let other = Seen {
        site: Some(SiteId(2)),
        ..auto(5, changes("e"))
    };
    let decision = decide(
        &[auto(1, changes("a")), auto(2, Diff::InLine), other],
        &BTreeMap::new(),
        &BTreeSet::new(),
        &[SITE].into(),
    );
    assert_eq!(decision.not_in_line, [SITE].into());
    assert_eq!(decision.members, ids(&[5]));
}
