//! Scope: The changes the provisioner wrote to NetBox and how far each has reached its devices
//! (`docs/auto-rollout.md`, „Stand pro Änderung“), from what the rounds saw of each device; not
//! the rounds themselves (`auto_rollout`) nor writing (`helper::write`).
//!
//! A change is kept with when it was written, who wrote it, for which resident and the devices
//! whose configuration it changes. Each round notes per device how it stood, as of the round's
//! start: a round that began before a change read the devices without it. A device seen in line
//! or applied after the change has it; until then the newest round after the change says why
//! not.
use super::{
    RoundDevice,
    decide::{self, Decision, Diff},
    hold::HoldReason,
};
use crate::topology::DeviceId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::SystemTime};

/// How many changes are kept, the newest.
pub const KEPT: usize = 200;

/// A change the provisioner wrote to NetBox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub id: u32,
    /// the `NetboxWrite` or `NetboxUndo` job that wrote it
    pub job: u32,
    pub at: SystemTime,
    /// who wrote it, as the job names them („Hans Meier for Familie Meier“)
    pub by: Box<str>,
    /// the resident (NetBox owner) it was written for; `None` for an admin's helper
    pub owner: Option<u32>,
    /// the RouterOS devices whose configuration it changes
    pub devices: Box<[RoundDevice]>,
}

/// How a round saw a device, as of its start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Observed {
    /// as NetBox has it
    InLine,
    /// applied by the round
    Applied,
    /// its changes wait for an admin
    Held(Box<[HoldReason]>),
    /// it would have gone, but another device held the round back, or the round paused (the
    /// precondition, no internet, a device busy)
    Waiting,
    /// not reached, or a radio partner not reached
    Deferred,
    /// the apply failed, or its site is in its error state
    Failed,
}

/// The newest round's view of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub at: SystemTime,
    pub seen: Observed,
}

/// What the rounds keep per device.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Devices {
    /// the newest round's view
    pub seen: BTreeMap<DeviceId, Observation>,
    /// the start of the newest round that saw it in line or applied it
    pub in_line: BTreeMap<DeviceId, SystemTime>,
}

impl Devices {
    /// Notes what the round started `at` saw of `device`.
    pub fn note(&mut self, device: DeviceId, at: SystemTime, seen: Observed) {
        if matches!(seen, Observed::InLine | Observed::Applied) {
            self.in_line.insert(device, at);
        }
        self.seen.insert(device, Observation { at, seen });
    }
}

/// What a round's `decision` over `seen` says of each device before applying: the members it
/// applies are left out, their outcome is the batch's.
pub fn observed(seen: &[decide::Seen], decision: &Decision) -> BTreeMap<DeviceId, Observed> {
    let held_back = !decision.held_by.is_empty() || !decision.held.is_empty();
    seen.iter()
        .filter_map(|s| {
            let automatic = s.site.is_some();
            let observed = match &s.diff {
                Diff::InLine => Observed::InLine,
                // a manual device's changes are an admin's to apply
                _ if !automatic => return None,
                Diff::Unreached => Observed::Deferred,
                Diff::Changes(_) if decision.deferred.contains(&s.device) => Observed::Deferred,
                Diff::Changes(_) => match decision.held.get(&s.device) {
                    Some(reasons) => Observed::Held(reasons.clone()),
                    None if held_back
                        || s.site
                            .is_some_and(|site| decision.not_in_line.contains(&site)) =>
                    {
                        Observed::Waiting
                    }
                    None => return None,
                },
            };
            Some((s.device, observed))
        })
        .collect()
}

/// How far a change has reached one of its devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum RolloutState {
    /// no round saw the device since
    Pending,
    /// the device has it
    RolledOut,
    /// it waits for an admin (`reasons`), for another device, or the rounds paused
    Waiting,
    /// the device wasn't reached
    Deferred,
    /// the apply failed, all went back; an admin clears the site's error
    Failed,
    /// the device doesn't roll out automatically: an admin applies it
    Manual,
}

/// How far a change written `at` has reached `device`; with the reasons it waits for an admin.
pub fn state_of(
    at: SystemTime,
    device: DeviceId,
    devices: &Devices,
    automatic: bool,
) -> (RolloutState, Box<[HoldReason]>) {
    if devices
        .in_line
        .get(&device)
        .is_some_and(|since| *since >= at)
    {
        return (RolloutState::RolledOut, Box::new([]));
    }
    if !automatic {
        return (RolloutState::Manual, Box::new([]));
    }
    match devices.seen.get(&device).filter(|seen| seen.at >= at) {
        None => (RolloutState::Pending, Box::new([])),
        Some(Observation { seen, .. }) => match seen {
            // seen in line later than the change is caught above
            Observed::InLine | Observed::Applied => (RolloutState::Pending, Box::new([])),
            Observed::Held(reasons) => (RolloutState::Waiting, reasons.clone()),
            Observed::Waiting => (RolloutState::Waiting, Box::new([])),
            Observed::Deferred => (RolloutState::Deferred, Box::new([])),
            Observed::Failed => (RolloutState::Failed, Box::new([])),
        },
    }
}

/// Keeps `change`, the newest [`KEPT`].
pub fn keep(changes: &mut Vec<Change>, change: Change) {
    changes.push(change);
    if changes.len() > KEPT {
        let drop = changes.len() - KEPT;
        changes.drain(..drop);
    }
}

#[cfg(test)]
mod test;
