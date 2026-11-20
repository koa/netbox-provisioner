//! Scope: What a resident changes in NetBox: the plan for its preview, saving it and how saving
//! went; not computing a plan (the helpers), nor what a resident sees (`graphql::resident`).
//!
//! Every change is a helper's plan (`helper`), allowed only on the resident's own objects
//! (`Topology::owned_by`): a resident for its owners, an admin for any (the support view,
//! `docs/concepts/self-service.md`). A WLAN's name, key and on/off (`helper::wlan`), the VLAN
//! of a port or socket (`helper::port_vlan`; a socket's is the switch port's behind the wall,
//! and nothing but the resident's devices may hang at it). Saving writes the plan as a `NetboxWrite` job of the
//! plan's device, as the admins' helpers do; the devices get it from the automatic rollout.
use super::{Named, PortKind, switch_port};
use crate::{
    Error,
    config::CONFIG,
    context::UserInfo,
    error_info::{ApiError, ErrorInfo},
    graphql::refetch,
    helper::{
        self, Helper, NetboxPlan, PlanStep, UpdatePortVlan, UpdateWirelessLan,
        port_vlan::PortVlanInput, wlan::WlanInput,
    },
    job::{self, JobKind, JobState},
    journal::Started,
    lock::LockAnyway,
    netbox,
    role::Role,
    topology::{
        FrontPortId, InterfaceId, OwnerId, Topology, TopologyHolder, VlanId, WlanId,
        access::{AccessTopology, device::DeviceAccess, front_port::FrontPortAccess},
        ownership::Owned,
    },
};
use async_graphql::{Context, Enum, InputObject, Object, OneofObject, SimpleObject};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

/// The owner each saving job was for: only its resident (or an admin) asks how it went.
static SAVED_FOR: LazyLock<Mutex<HashMap<u32, OwnerId>>> = LazyLock::new(Mutex::default);

/// A change of a resident.
#[derive(Debug, Clone, PartialEq, Eq, OneofObject)]
pub enum ResidentChange {
    /// name, key or on/off of its WLAN (`setWlanSsid`, `setWlanPsk`, `setWlanEnabled`)
    Wlan(WlanInput),
    /// the network of its port or socket (`setPortVlan`)
    PortVlan(PortVlanChange),
}

/// One of the resident's VLANs untagged at its port or socket, or none.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct PortVlanChange {
    /// a row of the resident's ports (`ResidentPort.id` with its `kind`)
    pub port: u32,
    pub kind: PortKind,
    /// none: no network there
    pub vlan: Option<u32>,
}

/// What saving a change would write, for the preview.
#[derive(SimpleObject, Debug, Clone)]
pub struct ResidentPlan {
    /// identifies the plan: saving refuses one that has become another by then
    fingerprint: String,
    /// the WLANs it changes
    wlans: Box<[UpdateWirelessLan]>,
    /// the ports whose VLAN it changes (a socket's: the switch port behind it)
    ports: Box<[UpdatePortVlan]>,
    /// the devices whose configuration changes: the plan's device, a WLAN group's APs (with
    /// RouterOS only)
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
pub(super) fn acting_for(
    context: &Context<'_>,
    topology: &Topology,
    resident: u32,
) -> Result<OwnerId, Error> {
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

/// The helper for `owner`'s `change`, refused for what isn't its own.
fn helper_for(
    topology: &Arc<Topology>,
    owner: OwnerId,
    change: ResidentChange,
) -> Result<Helper, Error> {
    let owned = topology.owned_by(owner);
    match change {
        ResidentChange::Wlan(change) => {
            if !owned.wlans.contains(&WlanId(change.wlan)) {
                return Err(Error::NotOwned(format!("WLAN {}", change.wlan).into()));
            }
            Ok(Helper::Wlan(change))
        }
        ResidentChange::PortVlan(change) => port_vlan_helper(topology, &owned, &change),
    }
}

/// The port whose VLAN `change` sets: the resident's own, or the switch port behind its
/// socket; its VLAN the resident's, and every device at the port's cable the resident's.
fn port_vlan_helper(
    topology: &Arc<Topology>,
    owned: &Owned,
    change: &PortVlanChange,
) -> Result<Helper, Error> {
    let not_owned = |what: String| Error::NotOwned(what.into());
    let port = match change.kind {
        PortKind::Interface => {
            if !owned.ports.contains(&InterfaceId(change.port)) {
                return Err(not_owned(format!("port {}", change.port)));
            }
            topology
                .get_interface_by_id(InterfaceId(change.port))
                .ok_or(Error::UnknownInterface(change.port))?
        }
        PortKind::Socket => {
            if !owned.sockets.contains(&FrontPortId(change.port)) {
                return Err(not_owned(format!("socket {}", change.port)));
            }
            let socket = FrontPortAccess::create(topology.clone(), FrontPortId(change.port));
            switch_port(&socket).ok_or_else(|| {
                let name = socket
                    .device()
                    .map(|device| {
                        format!("{} {}", device.name(), socket.name().unwrap_or_default())
                    })
                    .unwrap_or_default();
                Error::SocketWithoutPort(name.into())
            })?
        }
    };
    if let Some(vlan) = change.vlan
        && !owned.vlans.contains(&VlanId(vlan))
    {
        return Err(not_owned(format!("VLAN {vlan}")));
    }
    // a port cut off from its network cuts off what hangs at it: the resident's devices only
    if let Some(foreign) = port
        .connected_interfaces()
        .iter()
        .filter_map(|far| far.device())
        .find(|device| !owned.devices.contains(&device.id()))
    {
        return Err(not_owned(foreign.name().to_string()));
    }
    let device = port.device().ok_or(Error::UnknownInterface(change.port))?;
    Ok(Helper::PortVlan(PortVlanInput {
        device: device.id().0,
        port: AccessTopology::id(&port).0,
        vlan: change.vlan,
    }))
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
    let ports = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdatePortVlan(update) => Some(update.clone()),
            _ => None,
        })
        .collect();
    let device = plan.device();
    let aps = plan
        .steps
        .iter()
        .find_map(|step| match step {
            PlanStep::UpdateWirelessLan(update) => {
                helper::wlan::wlan(device.topology(), update.id).ok()
            }
            _ => None,
        })
        .and_then(|wlan| wlan.wlan_group())
        .map(|group| group.aps())
        .unwrap_or_default();
    // a port's VLAN tunneled over VXLAN: the device joining or leaving it changes the VTEPs
    // of every other device with it
    let vteps = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdatePortVlan(update) => Some(update),
            _ => None,
        })
        .flat_map(|update| update.vlan.iter().chain(&update.vlan_before))
        .flat_map(|vlan| vtep_peers(plan, VlanId(vlan.id)))
        .collect::<Vec<_>>();
    // only those the provisioner configures: a switch without RouterOS changes in NetBox only
    let mut seen = std::collections::BTreeSet::new();
    let devices = std::iter::once(device.clone())
        .chain(aps)
        .chain(vteps)
        .filter(|device| device.has_routeros() && seen.insert(device.id()))
        .map(|device| Named {
            id: device.id().0,
            name: device.name().into(),
        })
        .collect();
    ResidentPlan {
        fingerprint: plan.fingerprint_value(),
        wlans,
        ports,
        devices,
    }
}

/// The other devices with `vlan` over its VXLAN, when the plan's device joins or leaves it:
/// before and after the plan.
fn vtep_peers(plan: &NetboxPlan, vlan: VlanId) -> Vec<DeviceAccess> {
    let device = plan.device();
    let Ok(preview) = plan.preview_topology() else {
        return Vec::new();
    };
    let devices = |topology: &Arc<Topology>| {
        topology
            .list_vlans()
            .find(|other| other.id == vlan && other.vxlan().is_some())
            .map(|other| other.devices().into_vec())
            .unwrap_or_default()
    };
    let before = devices(device.topology());
    let after = devices(&preview);
    let has = |devices: &[DeviceAccess]| devices.iter().any(|d| d.id() == device.id());
    if has(&before) == has(&after) {
        return Vec::new();
    }
    before
        .into_iter()
        .chain(after)
        .filter(|other| other.id() != device.id())
        .collect()
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
    /// What `resident`'s change would write, for the preview.
    async fn change_plan(
        &self,
        context: &Context<'_>,
        resident: u32,
        change: ResidentChange,
    ) -> Result<ResidentPlan, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let plan = helper_for(&topology, owner, change)?
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
    /// Saves `resident`'s change to NetBox, if its plan is still the one shown
    /// (`ResidentPlan.fingerprint`); `savedChange` tells how it went.
    async fn save_change(
        &self,
        context: &Context<'_>,
        resident: u32,
        change: ResidentChange,
        fingerprint: String,
    ) -> Result<SavedChange, ApiError> {
        if !(CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some()) {
            return Err(Error::Netbox(netbox::NetboxError::WriteNotAllowed).into());
        }
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let helper = helper_for(&topology, owner, change)?;
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
