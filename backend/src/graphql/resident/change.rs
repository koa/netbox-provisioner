//! Scope: What a resident changes in NetBox: the plan for its preview, saving it and how saving
//! went; not computing a plan (the helpers), nor what a resident sees (`graphql::resident`).
//!
//! A resident saves its changes together (its drafts): one plan, each change computed on
//! NetBox as the ones before it leave it (`helper::sequence`), and checked there too. Every
//! change is a helper's plan (`helper`), allowed only on the resident's own objects
//! (`Topology::owned_by`): a resident for its owners, an admin for any (the support view,
//! `docs/self-service.md`). A WLAN's name, key and on/off (`helper::wlan`), the VLAN
//! of a port or socket (`helper::port_vlan`; a socket's is the switch port's behind the wall,
//! and nothing but the resident's devices may hang at it). One of its devices linked to its
//! port, socket or another of its devices (`helper::connect_device`, also to move the link to
//! another port of the device), or taken off its link (`helper::disconnect` at its Anschluss);
//! what is in the way at the port has to be its own, and no other's device may lose its way to
//! the management roots (`ManagementTree` before and after the whole plan), nor its own but
//! those taken off their link and behind them. Saving writes the plan as a `NetboxWrite` job of
//! the first change's device, as the admins' helpers do; the devices get it from the automatic
//! rollout.
use super::{Named, PortKind};
use crate::{
    Error, auto_rollout,
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
        sequence,
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
    /// name, key or on/off of its WLAN
    Wlan(WlanInput),
    /// the network of its port or socket
    PortVlan(PortVlanChange),
    /// one of its devices linked to its port or socket, or to a port of another of its devices
    /// (with `devicePort` the device's port it goes out of, also to change its Anschluss: the
    /// same port with another `devicePort`)
    ConnectDevice(ConnectDeviceChange),
    /// one of its devices, by id, taken off its link
    DisconnectDevice(u32),
    /// the room of one of its devices
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

/// `owner`'s plan of `changes`, one after the other, each refused for what isn't its own on
/// NetBox as the ones before leave it; refused too if it would cut off a device it mustn't.
async fn checked_plan(
    topology: &Arc<Topology>,
    owner: OwnerId,
    changes: Vec<ResidentChange>,
) -> Result<Checked, Error> {
    // a device taken off its link goes, and its own behind it: nothing else of the resident's
    let disconnected = changes
        .iter()
        .filter_map(|change| match change {
            ResidentChange::DisconnectDevice(device) => Some(DeviceId(*device)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut helpers = Vec::with_capacity(changes.len());
    let mut configured = Vec::new();
    let mut plan = None;
    for change in changes {
        let now = sequence::after(plan.as_ref(), topology)?;
        let helper = helper_for(&now, &now.owned_by(owner), change)?;
        // a room is NetBox's only: the device's configuration stays
        if !matches!(helper, Helper::DeviceLocation(_)) {
            configured.push(helper.device(&now)?);
        }
        plan = sequence::then(plan, &now, &helper).await?;
        helpers.push(helper);
    }
    let plan = plan.ok_or(Error::NothingToChange)?;
    let cuts_cables = plan
        .steps
        .iter()
        .any(|step| matches!(step, PlanStep::DeleteCable(_) | PlanStep::DeleteAddress(_)));
    if cuts_cables {
        keeps_reached(topology, &topology.owned_by(owner), &plan, &disconnected)?;
    }
    Ok(Checked {
        helpers,
        configured,
        plan,
    })
}

/// A resident's changes with their plan.
struct Checked {
    helpers: Vec<Helper>,
    /// the devices the changes are for whose configuration they change (a WLAN's controller,
    /// a port's device), not those a room is changed of
    configured: Vec<DeviceAccess>,
    plan: NetboxPlan,
}

/// The RouterOS devices reaching a management root before `plan` reach one after it too: all
/// of others, and of the resident's own all but those `disconnected` and behind them. The
/// whole plan counts, as the devices get it at once.
fn keeps_reached(
    topology: &Arc<Topology>,
    owned: &Owned,
    plan: &NetboxPlan,
    disconnected: &BTreeSet<DeviceId>,
) -> Result<(), Error> {
    let reached = |topology: &Arc<Topology>, tree: &ManagementTree| {
        topology
            .list_devices()
            .filter(DeviceAccess::has_routeros)
            .map(|device| device.id())
            .filter(|id| tree.is_root(*id) || tree.parent(*id).is_some())
            .collect::<BTreeSet<_>>()
    };
    let before = ManagementTree::of(topology, topology.management_proxy());
    let after = plan.preview_topology()?;
    let after_tree = ManagementTree::of(&after, after.management_proxy());
    for lost in reached(topology, &before).difference(&reached(&after, &after_tree)) {
        let own = owned.devices.contains(lost);
        if own && behind(&before, *lost, disconnected) {
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

/// Whether `device` is one of `devices` or reaches its management root through one of them.
fn behind(tree: &ManagementTree, device: DeviceId, devices: &BTreeSet<DeviceId>) -> bool {
    let mut seen = BTreeSet::new();
    let mut on_the_way = Some(device);
    while let Some(device) = on_the_way.filter(|device| seen.insert(*device)) {
        if devices.contains(&device) {
            return true;
        }
        on_the_way = tree.parent(device);
    }
    false
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

fn resident_plan(plan: &NetboxPlan, configured: &[DeviceAccess]) -> ResidentPlan {
    let mut wlans = Vec::new();
    let mut ports = Vec::new();
    let mut cables_added = Vec::new();
    let mut cables_removed = Vec::new();
    let mut rooms = Vec::new();
    for step in &plan.steps {
        match step {
            PlanStep::UpdateWirelessLan(update) => wlans.push(update.clone()),
            PlanStep::UpdatePortVlan(update) => ports.push(update.clone()),
            PlanStep::CreateCable(create) => cables_added.push(create.clone()),
            PlanStep::DeleteCable(delete) => cables_removed.push(delete.clone()),
            PlanStep::UpdateDeviceLocation(update) => rooms.push(update.clone()),
            _ => {}
        }
    }
    let devices = changed_devices(plan, configured)
        .into_iter()
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

/// The RouterOS devices whose configuration `plan` changes: `configured`, those the steps touch,
/// a changed WLAN's group's APs, VXLAN peers. A switch without RouterOS changes in NetBox only.
fn changed_devices(plan: &NetboxPlan, configured: &[DeviceAccess]) -> Vec<DeviceAccess> {
    let topology = plan.device().topology();
    let aps = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::UpdateWirelessLan(update) => helper::wlan::wlan(topology, update.id).ok(),
            _ => None,
        })
        .filter_map(|wlan| wlan.wlan_group())
        .flat_map(|group| group.aps());
    // a port's VLAN tunneled over VXLAN: a device joining or leaving it changes the VTEPs of
    // every other device with it
    let preview = plan.preview_topology().ok();
    let vteps = preview.iter().flat_map(|after| {
        plan.steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::UpdatePortVlan(update) => Some(update),
                _ => None,
            })
            .flat_map(move |update| {
                let device = DeviceId(update.interface.device.id);
                update
                    .vlan
                    .iter()
                    .chain(&update.vlan_before)
                    .flat_map(move |vlan| vtep_peers(topology, after, VlanId(vlan.id), device))
            })
    });
    let mut seen = BTreeSet::new();
    configured
        .iter()
        .cloned()
        .chain(plan.touched_devices())
        .chain(aps)
        .chain(vteps)
        .filter(|device| device.has_routeros() && seen.insert(device.id()))
        .collect()
}

/// The other devices with `vlan` over its VXLAN, when `device` joins or leaves it: before
/// (`topology`) and `after` the plan. Where `device` is no VTEP (a switch without RouterOS)
/// that names more devices than change, along with the one whose trunk to it does.
fn vtep_peers(
    topology: &Arc<Topology>,
    after: &Arc<Topology>,
    vlan: VlanId,
    device: DeviceId,
) -> Vec<DeviceAccess> {
    let devices = |topology: &Arc<Topology>| {
        topology
            .list_vlans()
            .find(|other| other.id == vlan && other.vxlan().is_some())
            .map(|other| other.devices().into_vec())
            .unwrap_or_default()
    };
    let before = devices(topology);
    let after = devices(after);
    let has = |devices: &[DeviceAccess]| devices.iter().any(|other| other.id() == device);
    if has(&before) == has(&after) {
        return Vec::new();
    }
    before
        .into_iter()
        .chain(after)
        .filter(|other| other.id() != device)
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
    /// What `resident`'s changes would write together, one after the other, for the preview.
    async fn change_plan(
        &self,
        context: &Context<'_>,
        resident: u32,
        changes: Vec<ResidentChange>,
    ) -> Result<ResidentPlan, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        let Checked {
            plan, configured, ..
        } = checked_plan(&topology, owner, changes).await?;
        Ok(resident_plan(&plan, &configured))
    }
    /// `resident`'s saved changes, the newest first, and how far each has reached its
    /// devices (the automatic rollout's state per device).
    async fn changes(
        &self,
        context: &Context<'_>,
        resident: u32,
    ) -> Result<Box<[auto_rollout::graphql::ChangeState]>, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        Ok(auto_rollout::graphql::changes(&topology, |change| {
            change.owner == Some(owner.0)
        }))
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
    /// Saves `resident`'s changes to NetBox together, if their plan is still the one shown
    /// (`ResidentPlan.fingerprint`); `savedChange` tells how it went.
    async fn save_change(
        &self,
        context: &Context<'_>,
        resident: u32,
        changes: Vec<ResidentChange>,
        fingerprint: String,
    ) -> Result<SavedChange, ApiError> {
        if !(CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some()) {
            return Err(Error::Netbox(netbox::NetboxError::WriteNotAllowed).into());
        }
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        let owner = acting_for(context, &topology, resident)?;
        // the checks on the plan as previewed; the write refuses a plan changed since
        let Checked {
            helpers,
            configured,
            ..
        } = checked_plan(&topology, owner, changes).await?;
        // the first change's device runs the job
        let device = helpers
            .first()
            .ok_or(Error::NothingToChange)?
            .device(&topology)?;
        let user = started_by(context, &topology, owner);
        let holder = self.topology.clone();
        let current: Arc<Topology> = topology.clone();
        let job = job::start(
            &device,
            JobKind::NetboxWrite,
            Started::By(user.clone()),
            |log| async move {
                let result = helper::write(&helpers, &current, &fingerprint, &user, &log).await;
                if let (Ok(plan), Some(job)) = (&result, log.job_id()) {
                    let devices = auto_rollout::configured(changed_devices(plan, &configured));
                    auto_rollout::note_change(job, &user, Some(owner.0), devices);
                }
                log.enter(crate::device_state::NetboxState::Refetching);
                refetch(&holder).await;
                result.map(drop)
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
