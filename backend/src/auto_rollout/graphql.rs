//! Scope: The automatic rollout as a site's page shows it; not the rounds (`auto_rollout`).
use super::{
    HeldDevice, RoundDevice, RoundDone, RoundOutcome, SiteError, SiteState, precondition::Unmet,
    state,
};
use crate::topology::SiteId;
use async_graphql::SimpleObject;
use std::time::SystemTime;

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

/// What `site` shows; noted, so a subscription answers again when it changes.
pub fn of_site(site: SiteId) -> SiteAutoRollout {
    crate::facts::note_auto_rollout();
    let state = state();
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
    }
}
