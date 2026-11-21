//! Scope: What a resident changes in NetBox: the plan for its preview, saving it and how saving
//! went; not computing a plan (the helpers), nor what a resident sees (`graphql::resident`).
//!
//! Every change is a helper's plan (`helper`), allowed only on the resident's own objects
//! (`Topology::owned_by`): a resident for its owners, an admin for any (the support view,
//! `docs/concepts/self-service.md`). A WLAN's name, key and on/off (`helper::wlan`), the VLAN
//! of a port or socket (`helper::port_vlan`; a socket's is the switch port's behind the wall,
//! and nothing but the resident's devices may hang at it). One of its devices linked to its
//! port, socket or another of its devices (`helper::connect_device`, also to move the link to
//! another port of the device), or taken off its link (`helper::disconnect` at its Anschluss);
//! what is in the way at the port has to be its own, and no other's device may lose its way to
//! the management roots (`ManagementTree` before and after the plan), nor its own when it
//! connects. Saving writes the plan as a `NetboxWrite` job of the plan's device, as the admins'
//! helpers do; the devices get it from the automatic rollout.
use super::{Named, PortKind};
use crate::{
    Error,
    config::CONFIG,
    error_info::{ApiError, ErrorInfo},
    graphql::{refetch, user_label},
    helper::{
        self, CreateCable, DeleteCable, Helper, NetboxPlan, PlanStep, UpdateDeviceLocation,
        UpdatePortVlan, UpdateWirelessLan,
        connect_device::{ConnectDeviceInput, uplink_port},
        device_location::DeviceLocationInput,
        disconnect::DisconnectInput,
        port_vlan::PortVlanInput,
        wlan::WlanInput,
    },
    job::{self, JobKind, JobState},
    journal::Started,
    lock::LockAnyway,
    netbox,
    role::Role,
    topology::{
        DeviceId, FrontPortId, InterfaceId, OwnerId, Topology, TopologyHolder, VlanId, WlanId,
        access::{
            AccessTopology, device::DeviceAccess, front_port::FrontPortAccess,
            interface::InterfaceAccess,
        },
        management::ManagementTree,
        ownership::{Belonging, Owned},
    },
};
use async_graphql::{Context, Enum, InputObject, Object, OneofObject, SimpleObject};
use std::{
    collections::{BTreeSet, HashMap},
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
    /// one of its devices linked to its port or socket, or to a port of another of its devices
    /// (`connectDevice`; with `devicePort` the device's port it goes out of, also to change its
    /// Anschluss: the same port with another `devicePort`)
    ConnectDevice(ConnectDeviceChange),
    /// one of its devices, by id, taken off its link (`disconnectDevice`)
    DisconnectDevice(u32),
    /// the room of one of its devices (`setDeviceRoom`)
    DeviceRoom(DeviceRoomChange),
}

/// The room (NetBox location) one of the resident's devices stands in: one of the sites it
/// has ports or devices in.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct DeviceRoomChange {
    pub device: u32,
    pub location: u32,
}

/// Where one of the resident's devices gets linked.
#[derive(Debug, Clone, PartialEq, Eq, InputObject)]
pub struct ConnectDeviceChange {
    pub device: u32,
    /// a row of the resident's ports (`ResidentPort.id` with its `kind`)
    pub port: u32,
    pub kind: PortKind,
    /// the device's port the cable goes into; none: its Anschluss, or its first free port
    pub device_port: Option<u32>,
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
    fingerprint: Box<str>,
    /// the WLANs it changes
    wlans: Box<[UpdateWirelessLan]>,
    /// the ports whose VLAN it changes (a socket's: the switch port behind it)
    ports: Box<[UpdatePortVlan]>,
    /// the cables it adds (a socket's: from the socket's front port)
    cables_added: Box<[CreateCable]>,
    /// the cables it takes away
    cables_removed: Box<[DeleteCable]>,
    /// the devices it puts in another room
    rooms: Box<[UpdateDeviceLocation]>,
    /// devices cut off until the devices have the change: those whose only link goes
    cut_off: Box<[Named]>,
    /// the devices whose configuration changes: those the steps touch, a WLAN group's APs,
    /// VXLAN peers (with RouterOS only)
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
        Err(Error::NotOwned {
            what: Belonging::Resident,
            id: resident,
        })
    }
}

/// The helper for the change of the resident owning `owned`, refused for what isn't its own.
fn helper_for(
    topology: &Arc<Topology>,
    owned: &Owned,
    change: ResidentChange,
) -> Result<Helper, Error> {
    match change {
        ResidentChange::Wlan(change) => {
            if !owned.wlans.contains(&WlanId(change.wlan)) {
                return Err(Error::NotOwned {
                    what: Belonging::Wlan,
                    id: change.wlan,
                });
            }
            Ok(Helper::Wlan(change))
        }
        ResidentChange::PortVlan(change) => port_vlan_helper(topology, owned, &change),
        ResidentChange::ConnectDevice(change) => connect_helper(topology, owned, &change),
        ResidentChange::DisconnectDevice(device) => disconnect_helper(topology, owned, device),
        ResidentChange::DeviceRoom(change) => room_helper(topology, owned, &change),
    }
}

/// The room of `change`: its device the resident's, the location in one of its sites.
fn room_helper(
    topology: &Arc<Topology>,
    owned: &Owned,
    change: &DeviceRoomChange,
) -> Result<Helper, Error> {
    own_device(topology, owned, change.device)?;
    let location = topology
        .locations()
        .get(&change.location)
        .ok_or(Error::UnknownLocation(change.location))?;
    if !location
        .site
        .is_some_and(|site| super::resident_sites(topology, owned).contains(&site))
    {
        return Err(Error::NotOwned {
            what: Belonging::Location,
            id: change.location,
        });
    }
    Ok(Helper::DeviceLocation(DeviceLocationInput {
        device: change.device,
        location: Some(change.location),
    }))
}

/// `owner`'s plan of `change`, refused for what isn't its own or would cut off a device it
/// mustn't.
async fn checked_plan(
    topology: &Arc<Topology>,
    owner: OwnerId,
    change: ResidentChange,
) -> Result<Checked, Error> {
    let owned = topology.owned_by(owner);
    // a resident connecting keeps its devices reached, one disconnecting cuts its own off
    let own_may_go = matches!(change, ResidentChange::DisconnectDevice(_));
    let helper = helper_for(topology, &owned, change)?;
    let plan = helper.plan(topology).await?;
    let cuts_cables = plan
        .steps
        .iter()
        .any(|step| matches!(step, PlanStep::DeleteCable(_) | PlanStep::DeleteAddress(_)));
    if cuts_cables {
        keeps_reached(topology, &owned, &plan, own_may_go)?;
    }
    Ok(Checked { helper, plan })
}

/// A resident's change with its plan.
struct Checked {
    helper: Helper,
    plan: NetboxPlan,
}

/// The RouterOS devices reaching a management root before `plan` reach one after it too: all
/// of others, and the resident's own unless `own_may_go`.
fn keeps_reached(
    topology: &Arc<Topology>,
    owned: &Owned,
    plan: &NetboxPlan,
    own_may_go: bool,
) -> Result<(), Error> {
    let reached = |topology: &Arc<Topology>| {
        let tree = ManagementTree::of(topology, topology.management_proxy());
        topology
            .list_devices()
            .filter(DeviceAccess::has_routeros)
            .map(|device| device.id())
            .filter(|id| tree.is_root(*id) || tree.parent(*id).is_some())
            .collect::<BTreeSet<_>>()
    };
    let after = plan.preview_topology()?;
    for lost in reached(topology).difference(&reached(&after)) {
        let own = owned.devices.contains(lost);
        if own && own_may_go {
            continue;
        }
        let name: Box<str> = topology
            .get_device_by_id(lost)
            .map(|device| device.name().into())
            .unwrap_or_default();
        return Err(match own {
            true => Error::CutOff(name),
            false => Error::ForeignDeviceCutOff(name),
        });
    }
    Ok(())
}

/// The owned device of id `device`, or why not.
fn own_device(topology: &Arc<Topology>, owned: &Owned, device: u32) -> Result<DeviceAccess, Error> {
    if !owned.devices.contains(&DeviceId(device)) {
        return Err(Error::NotOwned {
            what: Belonging::Device,
            id: device,
        });
    }
    topology
        .get_device_by_id(&DeviceId(device))
        .ok_or(Error::UnknownDevice(device))
}

/// A row of the resident's ports (`ResidentPort.id` with its `kind`): the interface, for a
/// socket the switch port behind it.
struct OwnedPort {
    port: InterfaceAccess,
    socket: Option<FrontPortAccess>,
}

/// The resident's port `id` of `kind`, or why not.
fn owned_port(
    topology: &Arc<Topology>,
    owned: &Owned,
    kind: PortKind,
    id: u32,
) -> Result<OwnedPort, Error> {
    match kind {
        PortKind::Interface => {
            if !owned.ports.contains(&InterfaceId(id)) {
                return Err(Error::NotOwned {
                    what: Belonging::Port,
                    id,
                });
            }
            let port = topology
                .get_interface_by_id(InterfaceId(id))
                .ok_or(Error::UnknownInterface(id))?;
            Ok(OwnedPort { port, socket: None })
        }
        PortKind::Socket => {
            if !owned.sockets.contains(&FrontPortId(id)) {
                return Err(Error::NotOwned {
                    what: Belonging::Socket,
                    id,
                });
            }
            let socket = FrontPortAccess::create(topology.clone(), FrontPortId(id));
            let port = socket.interface_behind().ok_or_else(|| {
                let name = socket
                    .device()
                    .map(|device| {
                        format!("{} {}", device.name(), socket.name().unwrap_or_default())
                    })
                    .unwrap_or_default();
                Error::SocketWithoutPort(name.into())
            })?;
            Ok(OwnedPort {
                port,
                socket: Some(socket),
            })
        }
    }
}

/// The link of `change`: to the resident's port, the switch port behind its socket, or a port
/// of another of its devices; what hangs there now has to be its own.
fn connect_helper(
    topology: &Arc<Topology>,
    owned: &Owned,
    change: &ConnectDeviceChange,
) -> Result<Helper, Error> {
    let device = own_device(topology, owned, change.device)?;
    if !device.has_routeros() {
        return Err(Error::NoRouterOs(device.name().into()));
    }
    let device_port = match change.device_port {
        Some(id) => topology
            .get_interface_by_id(InterfaceId(id))
            .filter(|port| port.device().is_some_and(|d| d.id() == device.id()))
            .ok_or_else(|| Error::NotOwned {
                what: Belonging::Port,
                id,
            })?,
        None => uplink_port(&device).ok_or_else(|| Error::NoUplinkPort(device.name().into()))?,
    };
    let at = owned_port(topology, owned, change.kind, change.port)?;
    // what is plugged in there now: at a socket's front, else at the port
    let in_the_way = match &at.socket {
        Some(socket) => socket.plugged_in(),
        None => at.port.connected_interfaces(),
    };
    if let Some(foreign) = in_the_way
        .iter()
        .filter_map(InterfaceAccess::device)
        .find(|other| other.id() != device.id() && !owned.devices.contains(&other.id()))
    {
        return Err(Error::ForeignDeviceAtPort(foreign.name().into()));
    }
    Ok(Helper::ConnectDevice(ConnectDeviceInput {
        port: at.port.id().0,
        socket: at
            .socket
            .as_ref()
            .map(|socket| AccessTopology::id(socket).0),
        device_port: device_port.id().0,
    }))
}

/// The link at the Anschluss of `device` out of NetBox; refused while a device not the
/// resident's hangs at one of its other ports (fed by it, or reached through it).
fn disconnect_helper(
    topology: &Arc<Topology>,
    owned: &Owned,
    device: u32,
) -> Result<Helper, Error> {
    let device = own_device(topology, owned, device)?;
    let port = uplink_port(&device)
        .filter(|port| port.cable().is_some())
        .ok_or_else(|| Error::NothingToDisconnect(device.name().into()))?;
    if let Some(foreign) = device
        .interfaces()
        .iter()
        .filter(|other| other.id() != port.id())
        .flat_map(|other| other.connected_interfaces().into_vec())
        .filter_map(|far| far.device())
        .find(|far| far.id() != device.id() && !owned.devices.contains(&far.id()))
    {
        return Err(Error::ForeignDeviceBehind(foreign.name().into()));
    }
    // the router at the other end writes it, as when it was connected
    let writer = port
        .connected_interfaces()
        .iter()
        .filter_map(InterfaceAccess::device)
        .find(DeviceAccess::has_routeros)
        .unwrap_or(device);
    Ok(Helper::Disconnect(DisconnectInput {
        device: writer.id().0,
        interface: port.id().0,
    }))
}

/// The port whose VLAN `change` sets: the resident's own, or the switch port behind its
/// socket; its VLAN the resident's, and every device at the port's cable the resident's.
fn port_vlan_helper(
    topology: &Arc<Topology>,
    owned: &Owned,
    change: &PortVlanChange,
) -> Result<Helper, Error> {
    let OwnedPort { port, .. } = owned_port(topology, owned, change.kind, change.port)?;
    if let Some(vlan) = change.vlan
        && !owned.vlans.contains(&VlanId(vlan))
    {
        return Err(Error::NotOwned {
            what: Belonging::Vlan,
            id: vlan,
        });
    }
    // a device linked there goes with its link (`disconnectDevice`), not with a network
    if let Some(linked) = port
        .connected_interfaces()
        .iter()
        .filter_map(|far| far.device())
        .find(DeviceAccess::has_routeros)
    {
        return Err(Error::DeviceAtPort(linked.name().into()));
    }
    // a port cut off from its network cuts off what hangs at it: the resident's devices only
    if let Some(foreign) = port
        .connected_interfaces()
        .iter()
        .filter_map(|far| far.device())
        .find(|device| !owned.devices.contains(&device.id()))
    {
        return Err(Error::ForeignDeviceAtPort(foreign.name().into()));
    }
    let device = port.device().ok_or(Error::UnknownInterface(change.port))?;
    Ok(Helper::PortVlan(PortVlanInput {
        device: device.id().0,
        port: AccessTopology::id(&port).0,
        vlan: change.vlan,
    }))
}

fn resident_plan(plan: &NetboxPlan) -> ResidentPlan {
    let mut wlans = Vec::new();
    let mut ports = Vec::new();
    let mut cables_added = Vec::new();
    let mut cables_removed = Vec::new();
    let mut rooms = Vec::new();
    // the devices a step changes something at (a created one's id is a preview id, which the
    // topology hasn't)
    let mut touched = Vec::<u32>::new();
    for step in &plan.steps {
        match step {
            PlanStep::UpdateWirelessLan(update) => wlans.push(update.clone()),
            PlanStep::UpdatePortVlan(update) => {
                touched.push(update.interface.device.id);
                ports.push(update.clone());
            }
            PlanStep::CreateCable(create) => {
                touched.extend([create.a.device.id, create.b.device.id]);
                cables_added.push(create.clone());
            }
            PlanStep::DeleteCable(delete) => {
                touched.push(delete.port.device.id);
                touched.extend(delete.far.iter().map(|far| far.device.id));
                cables_removed.push(delete.clone());
            }
            // a room is NetBox's only: the device's configuration stays
            PlanStep::UpdateDeviceLocation(update) => rooms.push(update.clone()),
            PlanStep::CreateInterface(create) => touched.push(create.device.id),
            PlanStep::CreateAddress(create) => touched.push(create.device.id),
            PlanStep::SetPrimaryIp(set) => touched.push(set.device.id),
            PlanStep::UpdateInterface(update) => touched.push(update.interface.device.id),
            PlanStep::DeleteAddress(delete) => touched.push(delete.interface.device.id),
            PlanStep::UpdatePort(update) => touched.push(update.interface.device.id),
            PlanStep::DeleteInterface(delete) => touched.push(delete.interface.device.id),
            PlanStep::CreateWirelessLink(link) => {
                touched.extend([link.a.device.id, link.b.device.id])
            }
            PlanStep::UpdateWirelessLink(link) => {
                touched.extend([link.a.device.id, link.b.device.id])
            }
            PlanStep::DeleteWirelessLink(link) => {
                touched.extend([link.a.device.id, link.b.device.id])
            }
            // a new device is no RouterOS device of the topology yet; a prefix or range changes
            // what the plan's own steps do with it
            PlanStep::CreateDevice(_)
            | PlanStep::CreatePrefix(_)
            | PlanStep::CreateIpRange(_)
            | PlanStep::DeletePrefix(_) => {}
        }
    }
    let device = plan.device();
    let topology = device.topology();
    let touched = touched
        .into_iter()
        .filter_map(|id| topology.get_device_by_id(&DeviceId(id)));
    let aps = wlans
        .iter()
        .find_map(|update| helper::wlan::wlan(topology, update.id).ok())
        .and_then(|wlan| wlan.wlan_group())
        .map(|group| group.aps())
        .unwrap_or_default();
    // a port's VLAN tunneled over VXLAN: the device joining or leaving it changes the VTEPs
    // of every other device with it
    let vteps = ports
        .iter()
        .flat_map(|update| update.vlan.iter().chain(&update.vlan_before))
        .flat_map(|vlan| vtep_peers(plan, VlanId(vlan.id)))
        .collect::<Vec<_>>();
    // only those the provisioner configures: a switch without RouterOS changes in NetBox only
    let mut seen = BTreeSet::new();
    let configured = rooms.len() < plan.steps.len();
    let devices = configured
        .then(|| device.clone())
        .into_iter()
        .chain(touched)
        .chain(aps)
        .chain(vteps)
        .filter(|device| device.has_routeros() && seen.insert(device.id()))
        .map(|device| Named {
            id: device.id().0,
            name: device.name().into(),
        })
        .collect();
    ResidentPlan {
        fingerprint: plan.fingerprint_value().into(),
        wlans: wlans.into(),
        ports: ports.into(),
        cables_added: cables_added.into(),
        cables_removed: cables_removed.into(),
        rooms: rooms.into(),
        cut_off: plan
            .cut_off
            .iter()
            .map(|device| Named {
                id: device.id,
                name: device.name.clone(),
            })
            .collect(),
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

/// The user (`user_label`, as the admins' jobs name it) and whom it acts for, for NetBox'
/// changelog and the job: „Hans Meier for Familie Meier“.
fn started_by(context: &Context<'_>, topology: &Topology, owner: OwnerId) -> Box<str> {
    let user = user_label(context);
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
        let Checked { plan, .. } = checked_plan(&topology, owner, change).await?;
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
        // the checks on the plan as previewed; the write refuses a plan changed since
        let Checked { helper, .. } = checked_plan(&topology, owner, change).await?;
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
        let mut saved_for = SAVED_FOR.lock_anyway();
        // a job the jobs no longer keep is asked for in vain
        saved_for.retain(|id, _| job::get(*id).is_some());
        saved_for.insert(job.job_id(), owner);
        Ok(saved_change(&job))
    }
}

#[cfg(test)]
mod test;
