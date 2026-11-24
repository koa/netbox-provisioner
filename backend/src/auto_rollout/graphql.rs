//! Scope: The automatic rollout as a site's page shows it; not the rounds (`auto_rollout`).
use super::{
    HeldDevice, RoundDevice, RoundDone, RoundOutcome, SiteError, SiteState,
    changes::{self, Change, RolloutState},
    hold::HoldReason,
    precondition::Unmet,
    state,
};
use crate::{
    graphql::resident::{PortKind, undo::UndoStep},
    topology::{DeviceId, SiteId, Topology, access::AccessTopology},
};
use async_graphql::SimpleObject;
use std::{sync::Arc, time::SystemTime};

/// The automatic rollout of a site with `auto_rollout`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SiteAutoRollout {
    /// `null` while no round saw the site yet
    pub seen: bool,
    /// its error state: its devices wait until an admin clears it
    pub error: Option<SiteAutoRolloutError>,
    /// the last round, over every automatic site
    pub last_round: Option<LastRound>,
    /// devices whose diff an admin released, applied with the next round (unless it changed)
    pub released: Box<[u32]>,
    /// devices holding back whose diff an admin may still release or (a manual one's) take
    /// over; one of the last round's `heldBy` missing here is taken over already
    pub holding: Box<[u32]>,
    /// the automatic devices, of every site, the precondition failed on at the last round:
    /// nothing rolls out until NetBox meets it again
    pub unmet: Box<[Unmet]>,
    /// the provisioner's changes to its devices, the newest first
    pub changes: Box<[ChangeState]>,
}

/// A change the provisioner wrote to NetBox and how far it has reached its devices.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ChangeState {
    pub id: u32,
    /// who wrote it
    pub by: Box<str>,
    /// the resident it was written for, by name; none for an admin's
    pub for_resident: Option<Box<str>>,
    /// the job that wrote it
    pub job: u32,
    /// seconds since
    pub age: u64,
    /// the RouterOS devices whose configuration it changes; none: NetBox only
    pub devices: Box<[ChangeDevice]>,
    /// it can be taken back (`undoChange`, a resident's change)
    pub undoable: bool,
    /// what it changed, from what takes it back (none for an admin's): its ports, WLANs and
    /// the devices it linked elsewhere or moved
    pub touched: Touched,
}

/// The elements a resident's change touched.
#[derive(Debug, Clone, Default, PartialEq, SimpleObject)]
pub struct Touched {
    /// ports whose network changed or that a device left
    pub ports: Box<[TouchedPort]>,
    pub wlans: Box<[u32]>,
    pub devices: Box<[u32]>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct TouchedPort {
    pub kind: PortKind,
    pub id: u32,
}

impl Touched {
    fn of(undo: &[UndoStep]) -> Touched {
        let mut ports = Vec::new();
        let mut wlans = Vec::new();
        let mut devices = Vec::new();
        for step in undo {
            match step {
                UndoStep::Wlan(wlan) => wlans.push(wlan.wlan),
                UndoStep::PortVlan(port) => ports.push(TouchedPort {
                    kind: port.kind,
                    id: port.port,
                }),
                UndoStep::ConnectDevice(link) => {
                    ports.push(TouchedPort {
                        kind: link.kind,
                        id: link.port,
                    });
                    devices.push(link.device);
                }
                UndoStep::DisconnectDevice(link) => devices.push(link.device),
                UndoStep::DeviceRoom(room) => devices.push(room.device),
            }
        }
        Touched {
            ports: ports.into(),
            wlans: wlans.into(),
            devices: devices.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ChangeDevice {
    pub device: RoundDevice,
    pub state: RolloutState,
    /// why it waits for an admin
    pub reasons: Box<[HoldReason]>,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct SiteAutoRolloutError {
    pub reason: SiteError,
    pub devices: Box<[RoundDevice]>,
    /// seconds since
    pub age: u64,
}

#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct LastRound {
    pub outcome: RoundOutcome,
    pub applied: Box<[RoundDevice]>,
    pub deferred: Box<[RoundDevice]>,
    pub held_by: Box<[RoundDevice]>,
    /// automatic devices whose changes wait for an admin, and why
    pub held: Box<[HeldDevice]>,
    /// seconds since
    pub age: u64,
}

fn age(at: SystemTime) -> u64 {
    at.elapsed().unwrap_or_default().as_secs()
}

/// The changes `wanted` gives, the newest first, with their devices' state in `topology`;
/// noted, so a subscription answers again when it changes.
pub fn changes(topology: &Arc<Topology>, wanted: impl Fn(&Change) -> bool) -> Box<[ChangeState]> {
    crate::facts::note_auto_rollout();
    let state = state();
    state
        .changes
        .iter()
        .rev()
        .filter(|change| wanted(change))
        .map(|change| ChangeState {
            id: change.id,
            by: change.by.clone(),
            for_resident: change.owner.and_then(|owner| {
                topology
                    .owners()
                    .find(|(id, _)| id.0 == owner)
                    .map(|(_, data)| data.name.clone())
            }),
            job: change.job,
            age: age(change.at),
            undoable: !change.undo.is_empty(),
            touched: Touched::of(&change.undo),
            devices: change
                .devices
                .iter()
                .map(|device| {
                    let id = DeviceId(device.id);
                    let automatic = topology
                        .get_device_by_id(&id)
                        .is_some_and(|device| device.rolls_out_automatically());
                    let (state, reasons) =
                        changes::state_of(change.at, id, &state.devices, automatic);
                    ChangeDevice {
                        device: device.clone(),
                        state,
                        reasons,
                    }
                })
                .collect(),
        })
        .collect()
}

/// What `site` of `topology` shows; noted, so a subscription answers again when it changes.
pub fn of_site(topology: &Arc<Topology>, site: SiteId) -> SiteAutoRollout {
    crate::facts::note_auto_rollout();
    let state = state();
    let of_site = |change: &Change| {
        change.devices.iter().any(|device| {
            topology
                .get_device_by_id(&DeviceId(device.id))
                .and_then(|device| device.site())
                .is_some_and(|other| other.id() == site)
        })
    };
    let site_state = state.sites.get(&site);
    SiteAutoRollout {
        seen: site_state.is_some(),
        error: match site_state {
            Some(SiteState::Error {
                reason,
                devices,
                at,
            }) => Some(SiteAutoRolloutError {
                reason: *reason,
                devices: devices.clone(),
                age: age(*at),
            }),
            _ => None,
        },
        last_round: state.last.map(
            |RoundDone {
                 at,
                 outcome,
                 applied,
                 deferred,
                 held_by,
                 held,
             }| LastRound {
                outcome,
                applied,
                deferred,
                held_by,
                held,
                age: age(at),
            },
        ),
        released: state.released_diffs.keys().map(|id| id.0).collect(),
        holding: state.holding.keys().map(|id| id.0).collect(),
        unmet: state.unmet,
        changes: changes(topology, of_site),
    }
}
