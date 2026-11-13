//! Scope: Where each device of a running rollout is, from the rollout's events, for the network
//! map; not carrying it out (`rollout::run`) nor the rollout page's table (the frontend's).
use super::run::RolloutDeviceState;
use crate::{
    device_state::UpdateState,
    job::{self, Event, JobKind},
};
use async_graphql::{Enum, SimpleObject};
use std::collections::HashMap;

/// Where a device of the rollout is.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Enum)]
pub enum RolloutNodeState {
    /// planned, not started yet
    Pending,
    /// it runs another job; tried again until that ends
    Waiting,
    /// its update job runs
    Updating,
    Done,
    Failed,
    /// its update job was cancelled
    Cancelled,
    /// left out by the operator
    Skipped,
}

/// A device of the running rollout on the map.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct RolloutNode {
    pub device: u32,
    pub state: RolloutNodeState,
    /// it reboots, or one it is behind or powered by does (`PlannedDevice::cut_by`)
    pub offline: bool,
}

/// The devices of the newest rollout while it runs; none without one running.
pub fn running() -> Box<[RolloutNode]> {
    match job::newest_of_network(JobKind::Rollout) {
        Some(job) if job.is_running() => nodes(&job.log().events()).into(),
        _ => Box::default(),
    }
}

/// The devices the plan updates, each in the state its last event gives: the pure part of
/// [`running`].
pub fn nodes(events: &[Event]) -> Vec<RolloutNode> {
    let Some(plan) = events.iter().rev().find_map(|event| match event {
        Event::RolloutPlanned(planned) => Some(&planned.plan),
        _ => None,
    }) else {
        return Vec::new();
    };
    let mut last = HashMap::new();
    for event in events {
        if let Event::RolloutDevice(device) = event {
            last.insert(device.device, device);
        }
    }
    let rebooting = |id: &u32| {
        last.get(id).is_some_and(|device| {
            device.state == RolloutDeviceState::Started
                && device.phase.is_some_and(|phase| {
                    matches!(
                        phase,
                        UpdateState::Rebooting | UpdateState::RebootingFirmware
                    )
                })
        })
    };
    plan.devices
        .iter()
        .filter(|device| device.skip.is_none())
        .map(|device| RolloutNode {
            device: device.id,
            state: match last.get(&device.id).map(|told| told.state) {
                None => RolloutNodeState::Pending,
                Some(RolloutDeviceState::Waiting) => RolloutNodeState::Waiting,
                Some(RolloutDeviceState::Started) => RolloutNodeState::Updating,
                Some(RolloutDeviceState::Done) => RolloutNodeState::Done,
                Some(RolloutDeviceState::Failed) => RolloutNodeState::Failed,
                Some(RolloutDeviceState::Cancelled) => RolloutNodeState::Cancelled,
                Some(RolloutDeviceState::Skipped) => RolloutNodeState::Skipped,
            },
            offline: rebooting(&device.id) || device.cut_by.iter().any(rebooting),
        })
        .collect()
}

#[cfg(test)]
mod test;
