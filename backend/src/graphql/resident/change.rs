//! Scope: What a resident changes in NetBox: the plan for its preview, saving it and how saving
//! went; not computing a plan (the helpers), nor what a resident sees (`graphql::resident`).
//!
//! Every change is a helper's plan (`helper`), allowed only on the resident's own objects
//! (`Topology::owned_by`): a resident for its owners, an admin for any (the support view,
//! `docs/concepts/self-service.md`). Saving writes the plan as a `NetboxWrite` job of the
//! plan's device, as the admins' helpers do; the devices get it from the automatic rollout.
use super::Named;
use crate::{
    Error,
    config::CONFIG,
    context::UserInfo,
    error_info::{ApiError, ErrorInfo},
    graphql::refetch,
    helper::{self, Helper, NetboxPlan, PlanStep, UpdateWirelessLan, wlan::WlanInput},
    job::{self, JobKind, JobState},
    journal::Started,
    lock::LockAnyway,
    netbox,
    role::Role,
    topology::{OwnerId, Topology, TopologyHolder, WlanId},
};
use async_graphql::{Context, Enum, Object, SimpleObject};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

/// The owner each saving job was for: only its resident (or an admin) asks how it went.
static SAVED_FOR: LazyLock<Mutex<HashMap<u32, OwnerId>>> = LazyLock::new(Mutex::default);

/// What saving a change would write, for the preview.
#[derive(SimpleObject, Debug, Clone)]
pub struct ResidentPlan {
    /// identifies the plan: saving refuses one that has become another by then
    fingerprint: String,
    /// the WLANs it changes
    wlans: Box<[UpdateWirelessLan]>,
    /// the devices whose configuration changes: the WLAN group's controller and APs
    devices: Box<[Named]>,
}

/// How saving a change stands.
#[derive(Enum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaveState {
    Saving,
    /// in NetBox; the devices get it from the automatic rollout
    Saved,
    Failed,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct SavedChange {
    /// the job writing it, for `savedChange`
    job: u32,
    state: SaveState,
    /// why it failed
    error: Option<ErrorInfo>,
}

/// The owner a request may act for: a resident its own, an admin any NetBox names.
fn acting_for(context: &Context<'_>, topology: &Topology, resident: u32) -> Result<OwnerId, Error> {
    let owner = OwnerId(resident);
    let allowed = match context.data_opt::<Role>() {
        Some(Role::Admin) => topology.owners().any(|(id, _)| id == owner),
        Some(Role::Resident { owners }) => owners.contains(&owner),
        Some(Role::Nobody) | None => false,
    };
    if allowed {
        Ok(owner)
    } else {
        Err(Error::NotOwned(format!("resident {resident}").into()))
    }
}

/// The helper for `change` of `owner`'s WLAN, refused for another's.
fn wlan_helper(topology: &Topology, owner: OwnerId, change: WlanInput) -> Result<Helper, Error> {
    if !topology
        .owned_by(owner)
        .wlans
        .contains(&WlanId(change.wlan))
    {
        return Err(Error::NotOwned(format!("WLAN {}", change.wlan).into()));
    }
    Ok(Helper::Wlan(change))
}

fn resident_plan(plan: &NetboxPlan) -> ResidentPlan {
    let wlans = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdateWirelessLan(update) => Some(update.clone()),
            _ => None,
        })
        .collect();
    let controller = plan.device();
    let aps = plan
        .steps
        .iter()
        .find_map(|step| match step {
            PlanStep::UpdateWirelessLan(update) => {
                helper::wlan::wlan(controller.topology(), update.id).ok()
            }
            _ => None,
        })
        .and_then(|wlan| wlan.wlan_group())
        .map(|group| group.aps())
        .unwrap_or_default();
    let devices = std::iter::once(controller.clone())
        .chain(aps)
        .map(|device| Named {
            id: device.id().0,
            name: device.name().into(),
        })
        .collect();
    ResidentPlan {
        fingerprint: plan.fingerprint_value(),
        wlans,
        devices,
    }
}

/// The user and whom it acts for, for NetBox' changelog and the job: „meier for Familie
/// Meier“.
fn started_by(context: &Context<'_>, topology: &Topology, owner: OwnerId) -> Box<str> {
    let user = context
        .data_opt::<UserInfo>()
        .and_then(|user| {
            user.preferred_username
                .as_deref()
                .map(str::to_string)
                .or_else(|| user.name.clone())
        })
        .unwrap_or_else(|| "unknown user".into());
    let name = topology
        .owners()
        .find(|(id, _)| *id == owner)
        .map(|(_, data)| data.name.to_string())
        .unwrap_or_default();
    format!("{user} for {name}").into()
}

fn saved_change(job: &job::Job) -> SavedChange {
    let outcome = job.outcome();
    let state = match outcome.state {
        JobState::Running => SaveState::Saving,
        JobState::Done => SaveState::Saved,
        JobState::Failed | JobState::Cancelled => SaveState::Failed,
    };
    SavedChange {
        job: job.job_id(),
        state,
        error: outcome.error,
    }
}

pub struct QueryChange {
    pub(super) topology: TopologyHolder,
}

impl QueryChange {
    pub fn new(topology: TopologyHolder) -> Self {
        QueryChange { topology }
    }
}

#[Object]
impl QueryChange {
    /// What changing `resident`'s WLAN would write, for the preview.
    async fn wlan_plan(
        &self,
        context: &Context<'_>,
        resident: u32,
        change: WlanInput,
    ) -> Result<ResidentPlan, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let plan = wlan_helper(&topology, owner, change)?
            .plan(&topology)
            .await?;
        Ok(resident_plan(&plan))
    }
    /// How saving the change of job `job` went, for its resident or an admin.
    async fn saved_change(
        &self,
        context: &Context<'_>,
        job: u32,
    ) -> Result<Option<SavedChange>, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let Some(owner) = SAVED_FOR.lock_anyway().get(&job).copied() else {
            return Ok(None);
        };
        acting_for(context, &topology, owner.0)?;
        Ok(job::get(job).map(|job| saved_change(&job)))
    }
}

pub struct MutationResident {
    topology: TopologyHolder,
}

impl MutationResident {
    pub fn new(topology: TopologyHolder) -> Self {
        MutationResident { topology }
    }
}

#[Object]
impl MutationResident {
    /// Saves the change of `resident`'s WLAN to NetBox, if its plan is still the one shown
    /// (`ResidentPlan.fingerprint`); `savedChange` tells how it went.
    async fn save_wlan(
        &self,
        context: &Context<'_>,
        resident: u32,
        change: WlanInput,
        fingerprint: String,
    ) -> Result<SavedChange, ApiError> {
        if !(CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some()) {
            return Err(Error::Netbox(netbox::NetboxError::WriteNotAllowed).into());
        }
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let helper = wlan_helper(&topology, owner, change)?;
        let device = helper.device(&topology)?;
        let user = started_by(context, &topology, owner);
        let holder = self.topology.clone();
        let current: Arc<Topology> = topology.clone();
        let job = job::start(
            &device,
            JobKind::NetboxWrite,
            Started::By(user.clone()),
            |log| async move {
                let result = helper::write(&helper, &current, &fingerprint, &user, &log).await;
                log.enter(crate::device_state::NetboxState::Refetching);
                refetch(&holder).await;
                result
            },
        )?;
        SAVED_FOR.lock_anyway().insert(job.job_id(), owner);
        Ok(saved_change(&job))
    }
}

#[cfg(test)]
mod test;
