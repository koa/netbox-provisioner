//! Scope: What a round of the automatic rollout applies, from what it read of the devices; no
//! reading, applying or keeping (`auto_rollout`).
//!
//! The rules (`docs/auto-rollout.md`):
//! - an automatic device not reached is passed over and gets its changes later, the other ends
//!   of its radio links too: a change to one end can cut the link;
//! - a site seen automatic for the first time with changes goes into its error state instead:
//!   switched on, everything that collected before would go out with the first change;
//! - a manual device next to automatic ones whose diff changed since the round before holds
//!   the whole round back until an admin releases it (then it is applied with them) or applies
//!   it by hand: no partial rollout;
//! - so does an automatic device with a reason to wait for an admin (`hold`), until that diff
//!   is released;
//! - a release counts for the diff it was given for (its fingerprint) only;
//! - every other automatic device with changes is applied, all together.
use super::hold::HoldReason;
use crate::topology::{DeviceId, SiteId};
use std::collections::{BTreeMap, BTreeSet};

/// The changes an apply would make on a device, as a hash of its script: equal scripts, equal
/// fingerprints, also across restarts.
pub type Fingerprint = Box<str>;

/// What a round read of one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diff {
    /// not reached, or its config couldn't be read or generated
    Unreached,
    /// as NetBox has it
    InLine,
    /// an apply would change it
    Changes(Fingerprint),
}

/// One device a round looked at: an automatic one, or a manual one next to them.
#[derive(Debug, Clone)]
pub struct Seen {
    pub device: DeviceId,
    /// `None` for a manual device
    pub site: Option<SiteId>,
    pub diff: Diff,
    /// the other ends of its radio links
    pub partners: Box<[DeviceId]>,
    /// why its changes wait for an admin (an automatic device)
    pub hold: Box<[HoldReason]>,
}

/// What the round does.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decision {
    /// applied together
    pub members: BTreeSet<DeviceId>,
    /// automatic devices with changes that wait: not reached, or a radio partner not reached
    pub deferred: BTreeSet<DeviceId>,
    /// manual devices whose diff changed, holding the round back
    pub held_by: BTreeSet<DeviceId>,
    /// automatic devices whose changes wait for an admin, holding the round back
    pub held: BTreeMap<DeviceId, Box<[HoldReason]>>,
    /// sites seen automatic for the first time with changes on their devices
    pub not_in_line: BTreeSet<SiteId>,
    /// what is kept of the manual devices for the next round
    pub baselines: BTreeMap<DeviceId, Fingerprint>,
}

/// The decision of a round over `seen`, with the manual devices' diffs of the round before
/// (`baselines`), the diffs an admin released and the sites seen for the first time.
pub fn decide(
    seen: &[Seen],
    baselines: &BTreeMap<DeviceId, Fingerprint>,
    released: &BTreeMap<DeviceId, Fingerprint>,
    new_sites: &BTreeSet<SiteId>,
) -> Decision {
    let mut decision = Decision::default();
    let unreached: BTreeSet<DeviceId> = seen
        .iter()
        .filter(|seen| seen.diff == Diff::Unreached)
        .map(|seen| seen.device)
        .collect();
    for seen in seen {
        match (&seen.site, &seen.diff) {
            (None, Diff::Changes(fingerprint)) => {
                if released.get(&seen.device) == Some(fingerprint) {
                    decision.members.insert(seen.device);
                    decision.baselines.insert(seen.device, fingerprint.clone());
                } else if baselines.get(&seen.device) == Some(fingerprint) {
                    decision.baselines.insert(seen.device, fingerprint.clone());
                } else {
                    // holds back until released or applied by hand: what it had stays
                    decision.held_by.insert(seen.device);
                    if let Some(before) = baselines.get(&seen.device) {
                        decision.baselines.insert(seen.device, before.clone());
                    }
                }
            }
            (None, Diff::Unreached) => {
                if let Some(before) = baselines.get(&seen.device) {
                    decision.baselines.insert(seen.device, before.clone());
                }
            }
            (None, Diff::InLine) => {}
            (Some(site), Diff::Changes(_)) if new_sites.contains(site) => {
                decision.not_in_line.insert(*site);
            }
            (Some(_), Diff::Changes(fingerprint)) => {
                if seen.partners.iter().any(|p| unreached.contains(p)) {
                    decision.deferred.insert(seen.device);
                } else if !seen.hold.is_empty() && released.get(&seen.device) != Some(fingerprint) {
                    decision.held.insert(seen.device, seen.hold.clone());
                } else {
                    decision.members.insert(seen.device);
                }
            }
            (Some(_), Diff::Unreached) => {
                decision.deferred.insert(seen.device);
            }
            (Some(_), Diff::InLine) => {}
        }
    }
    // a site just switched on stays out as a whole
    let in_new: BTreeSet<DeviceId> = seen
        .iter()
        .filter(|seen| {
            seen.site
                .is_some_and(|site| decision.not_in_line.contains(&site))
        })
        .map(|seen| seen.device)
        .collect();
    decision.members.retain(|device| !in_new.contains(device));
    decision.deferred.retain(|device| !in_new.contains(device));
    decision.held.retain(|device, _| !in_new.contains(device));
    // a member whose partner was passed over waits with it, along a chain of links too
    loop {
        let waiting: Vec<DeviceId> = seen
            .iter()
            .filter(|seen| decision.members.contains(&seen.device))
            .filter(|seen| seen.partners.iter().any(|p| decision.deferred.contains(p)))
            .map(|seen| seen.device)
            .collect();
        if waiting.is_empty() {
            break;
        }
        for device in waiting {
            decision.members.remove(&device);
            decision.deferred.insert(device);
        }
    }
    if !decision.held_by.is_empty() || !decision.held.is_empty() {
        decision.members.clear();
    }
    decision
}

#[cfg(test)]
mod test;
