use crate::{
    Error,
    config::CONFIG,
    device_state::{DeviceModeState, NetboxState, PowerCycleState, RebootState, UpdateState},
    error_info::ApiError,
    device::{
        AccessibleDevice, Connecting, bootstrap, device_mode, partitions,
        power::PowerChain,
        update::{self, DeviceLink},
        update_fallback,
        watchdog,
    },
    context::UserInfo,
    graphql::scalars::ScalarMac,
    helper::{self, Helper, NetboxPlan},
    job::{self, Job, JobKind, JobSubject, Log},
    netbox,
    saved_job::{self, SavedWork},
    topology::{
        DeviceId, SiteId, TopologyHolder,
        access::{AdhocCredentials, device::DeviceAccess, site::SiteAccess},
        management::ManagementTree,
    },
};
use std::{sync::Arc, time::SystemTime};
use async_graphql::{
    Context, EmptyMutation, EmptySubscription, Guard, Object, Schema, SimpleObject,
};
pub mod scalars;
pub type AuthenticatedGraphqlSchema =
    Schema<QueryAuthenticated, MutationAuthenticated, EmptySubscription>;
pub type AnonymousGraphqlSchema = Schema<QueryAnonymous, EmptyMutation, EmptySubscription>;

pub struct QueryAuthenticated {
    topology: TopologyHolder,
}
pub struct QueryAnonymous;
pub struct MutationAuthenticated {
    topology: TopologyHolder,
}

pub fn create_schema(topology: TopologyHolder) -> AuthenticatedGraphqlSchema {
    Schema::build(
        QueryAuthenticated {
            topology: topology.clone(),
        },
        MutationAuthenticated { topology },
        EmptySubscription,
    )
    .finish()
}
pub fn create_anonymous_schema() -> AnonymousGraphqlSchema {
    Schema::build(QueryAnonymous, EmptyMutation, EmptySubscription).finish()
}

#[Object]
impl QueryAuthenticated {
    async fn topology(&self) -> TopologyHolder {
        self.topology.clone()
    }
    async fn list_credentials(&self) -> Box<[&str]> {
        CONFIG
            .mikrotik_credentials
            .keys()
            .map(Box::as_ref)
            .collect()
    }
    /// Whether writing to devices is allowed (`allow_apply`): apply, setup, update and reboot;
    /// without it the UI offers them disabled instead of failing on the click.
    async fn allow_apply(&self) -> bool {
        CONFIG.allow_apply
    }
    /// NetBox' web UI, ending in `/`, for links to the objects of findings.
    async fn netbox_url(&self) -> String {
        netbox::web_url()
    }
    /// Whether the helpers may write to NetBox (`allow_netbox_write` and
    /// `netbox_write_token`); without it they show their plan only.
    async fn allow_netbox_write(&self) -> bool {
        CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some()
    }
    /// What `helper` would write to NetBox, with its preview.
    async fn netbox_plan(&self, helper: Helper) -> Result<NetboxPlan, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        Ok(helper.plan(&topology).await?)
    }
    /// The device types, roles and WLAN groups the helper „Neues Gerät“ offers.
    async fn new_device_options(&self) -> Result<helper::new_device::NewDeviceOptions, ApiError> {
        let topology = self.topology.current().await.ok_or(Error::NoTopology)?;
        Ok(helper::new_device::options(&topology).await?)
    }
    /// The device types and roles the helper „Lease übernehmen“ offers for the client.
    async fn lease_takeover_options(
        &self,
    ) -> Result<helper::lease_takeover::LeaseTakeoverOptions, ApiError> {
        Ok(helper::lease_takeover::options().await?)
    }
    async fn job(&self, id: u32) -> Option<Arc<Job>> {
        job::get(id)
    }
    /// The jobs of a device or a site (all without either), newest first; those still running
    /// and the last finished ones. A device's include those working through it (a setup over
    /// it as the neighbor, `deviceId` the device set up).
    async fn jobs(&self, device_id: Option<u32>, site_id: Option<u32>) -> Vec<Arc<Job>> {
        job::list(
            device_id
                .map(|id| JobSubject::Device(DeviceId(id)))
                .or(site_id.map(|id| JobSubject::Site(SiteId(id)))),
        )
    }
    /// The jobs saved before the restart that didn't go on: their device is gone from NetBox,
    /// or the file couldn't be read. They stay until discarded (`discardSavedJob`).
    async fn saved_jobs(&self) -> Vec<crate::saved_job::Waiting> {
        crate::saved_job::waiting()
    }
}
/// The watchdogs of others that may fire while a device updates (`suspendWatchdogs`), and
/// the addresses its suppliers' PoE ports ping.
struct Watchers {
    watchers: Vec<watchdog::Watcher>,
    powered: Vec<std::net::IpAddr>,
}

impl Watchers {
    fn of(device: &DeviceAccess, suspend: bool) -> Self {
        Self {
            watchers: match suspend {
                true => watchdog::watchers(device, &management_tree(device)),
                false => Vec::new(),
            },
            powered: watchdog::addresses(device),
        }
    }
    /// Connects to each and suspends its watchdogs.
    async fn suspend(self, log: &Log) -> Result<Vec<watchdog::Suspended<AccessibleDevice>>, Error> {
        if !self.watchers.is_empty() {
            log.enter(UpdateState::SuspendingWatchdogs);
        }
        let mut links = Vec::new();
        for watchdog::Watcher { device, supplier } in self.watchers {
            let link = connect_for_job(&device, None, None, None, log).await;
            if matches!(link, Err(Error::Cancelled)) {
                return Err(Error::Cancelled);
            }
            links.push(watchdog::WatcherLink {
                device: device.name().into(),
                supplier,
                powered: self.powered.clone(),
                link,
            });
        }
        watchdog::suspend_all(links, log).await
    }
}

/// Sets the watchdogs [`Watchers::suspend`] suspended again, whatever the job came to.
async fn restore_watchdogs(suspended: Vec<watchdog::Suspended<AccessibleDevice>>, log: &Log) {
    if !suspended.is_empty() {
        log.enter(UpdateState::RestoringWatchdogs);
    }
    watchdog::restore_all(suspended, log).await;
}

/// Fetches the topology after a write to NetBox, so the next page shows it.
async fn refetch(topology: &TopologyHolder) {
    if let Err(error) = topology.fetch().await {
        log::error!("Cannot fetch the topology after writing to NetBox: {error}");
    }
}

impl MutationAuthenticated {
    async fn device(&self, id: u32) -> Result<DeviceAccess, Error> {
        self.topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))
    }
    async fn site(&self, id: u32) -> Result<SiteAccess, Error> {
        self.topology.site(SiteId(id)).await.ok_or(Error::UnknownSite(id))
    }
}

/// Lets a mutation that writes to devices through only with `allow_apply`; every such
/// mutation carries it, the work below doesn't ask again.
struct WritesAllowed;
impl Guard for WritesAllowed {
    async fn check(&self, _ctx: &Context<'_>) -> async_graphql::Result<()> {
        if CONFIG.allow_apply {
            Ok(())
        } else {
            Err(ApiError(Error::ApplyNotAllowed).into())
        }
    }
}

/// Lets a helper's write to NetBox through only with `allow_netbox_write` and
/// `netbox_write_token`. Who may use which helper (the permission classes of
/// `docs/concepts/config-helper.md`) isn't told apart yet: every user of `auth_users` may.
struct NetboxWritesAllowed;
impl Guard for NetboxWritesAllowed {
    async fn check(&self, _ctx: &Context<'_>) -> async_graphql::Result<()> {
        if CONFIG.allow_netbox_write && CONFIG.netbox_write_token.is_some() {
            Ok(())
        } else {
            Err(ApiError(Error::Netbox(netbox::NetboxError::WriteNotAllowed)).into())
        }
    }
}

/// Connects to `device` for a job, which shows where it got to: a connection that takes long
/// (the NetBox address first, then each neighbor's tunnel) is otherwise a job without events.
pub(crate) async fn connect_for_job(
    device: &DeviceAccess,
    target: Option<String>,
    credential_name: Option<Box<str>>,
    adhoc_credentials: Option<AdhocCredentials>,
    log: &Log,
) -> Result<AccessibleDevice, Error> {
    log.emit(Connecting {
        device: device.name().into(),
    });
    let link = log
        .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
        .await??
        .ok_or(Error::MissingCredentials)?;
    log.emit(link.connected_device());
    Ok(link)
}

/// Goes on with the jobs saved before this start whose device NetBox has (`saved_job`), each
/// as a job of its own, a batch's devices together; the others stay in `savedJobs`. Called
/// once, after the topology is read.
pub async fn resume_saved_jobs(topology: &TopologyHolder) {
    let waiting = saved_job::waiting();
    let mut batches = std::collections::BTreeSet::new();
    for entry in &waiting {
        let Ok(saved) = &entry.saved else {
            continue;
        };
        let Some(device) = topology.devices_by_id(entry.device).await else {
            saved_job::hold_back(entry.device, saved_job::HeldBack::DeviceGone);
            continue;
        };
        let after = SystemTime::now().duration_since(saved.saved_at).unwrap_or_default();
        let started = match &saved.work {
            SavedWork::Apply { apply, .. } if apply.batch.is_some() => {
                if let Some(batch) = &apply.batch {
                    batches.insert(batch.devices.clone());
                }
                continue;
            }
            SavedWork::Apply { apply, .. } => {
                let (saved, apply) = (saved.clone(), apply.clone());
                saved_job::take(entry.device);
                job::start(&device.clone(), JobKind::Apply, |log| async move {
                    log.emit(job::Resumed { after: after.into() });
                    if let Some(hold) = &apply.hold {
                        crate::device::radio_hold::RadioHold::announce_saved(device.name(), hold, &log);
                    }
                    log.keep(saved);
                    // saved taking the fallback away too: that only follows a look at the
                    // marks, which comes first again
                    log.resume_in(crate::device_state::ApplyState::ScriptRunning);
                    resume_apply(&device, &apply, &log).await
                })
            }
            // the update goes on in a later step; until then it ends as before
            SavedWork::Update { .. } => {
                saved_job::take(entry.device);
                saved_job::remove(entry.device);
                continue;
            }
        };
        if let Err(error) = started {
            log::warn!("Cannot go on with the job of device {}: {error}", entry.device.0);
        }
    }
    for batch in batches {
        resume_batch(topology, &waiting, &batch).await;
    }
}

/// A batch saved before the restart, its devices `batch` ([`resume_saved_jobs`]): all of
/// them saved and in NetBox, they go on together; else none does, and their fallbacks take
/// all of it back.
async fn resume_batch(topology: &TopologyHolder, waiting: &[saved_job::Waiting], batch: &[DeviceId]) {
    let mut members = Vec::new();
    for id in batch {
        let saved = waiting.iter().find(|entry| entry.device == *id).and_then(|entry| entry.saved.as_ref().ok());
        let apply = saved.and_then(|saved| match &saved.work {
            SavedWork::Apply { apply, .. } if apply.batch.as_ref().is_some_and(|b| b.devices == batch) => Some(apply),
            _ => None,
        });
        match (saved, apply, topology.devices_by_id(*id).await) {
            (Some(saved), Some(apply), Some(device)) => members.push((device, saved.clone(), (**apply).clone())),
            _ => {
                for id in batch {
                    saved_job::hold_back(*id, saved_job::HeldBack::BatchIncomplete);
                }
                return;
            }
        }
    }
    let Some(delay) = members.first().and_then(|(_, _, apply)| apply.batch.as_ref()).map(|batch| batch.delay) else {
        return;
    };
    let fires = members
        .iter()
        .map(|(_, _, apply)| saved_job::instant(apply.script.fires))
        .min()
        .unwrap_or_else(std::time::Instant::now);
    for id in batch {
        saved_job::take(*id);
    }
    let devices: Vec<DeviceAccess> = members.iter().map(|(device, _, _)| device.clone()).collect();
    let started = job::start_all(&devices, JobKind::Apply, |logs| async move {
        for (log, (device, saved, apply)) in logs.iter().zip(&members) {
            let after = SystemTime::now().duration_since(saved.saved_at).unwrap_or_default();
            log.emit(job::Resumed { after: after.into() });
            if let Some(hold) = &apply.hold {
                crate::device::radio_hold::RadioHold::announce_saved(device.name(), hold, log);
            }
            log.keep(saved.clone());
            log.resume_in(crate::device_state::ApplyState::ScriptRunning);
        }
        let members = members.into_iter().map(|(device, _, apply)| (device, apply)).collect();
        crate::device::batch::resume(members, logs, fires, delay).await
    });
    if let Err(error) = started {
        log::warn!("Cannot go on with the batch of devices {batch:?}: {error}");
    }
}

/// An apply saved before the restart ([`resume_saved_jobs`]): connects until the device
/// answers, the script may have cut it off for a while. Not back before the fallback would
/// have brought it back, it ends as an apply not reached does, the fallback left.
async fn resume_apply(device: &DeviceAccess, saved: &saved_job::SavedApply, log: &Log) -> Result<(), Error> {
    use crate::device::ros::graphql::{COME_BACK, FallbackLeftReason, fallback_left, give_up};
    let fires = saved_job::instant(saved.script.fires);
    let come_back = fires + COME_BACK;
    let link = loop {
        match connect_for_job(device, None, saved.credentials.clone(), None, log).await {
            Ok(link) => break link,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) if std::time::Instant::now() < come_back => {
                log.unless_cancelled(tokio::time::sleep(RESUME_RETRY)).await?;
            }
            Err(error) => {
                log.enter(crate::device_state::ApplyState::NotReached);
                if saved.script.armed && std::time::Instant::now() < give_up(fires) {
                    fallback_left(log, fires, FallbackLeftReason::NotReached, None);
                }
                if let Some(hold) = &saved.hold {
                    crate::device::radio_hold::RadioHold::leave_saved(device.name(), hold, log);
                }
                return Err(error);
            }
        }
    };
    link.resume_apply(saved, log).await
}

/// How often a job going on after a restart tries to reach its device.
const RESUME_RETRY: std::time::Duration = std::time::Duration::from_secs(10);

/// The way of `device`'s watchdogs to the management roots, the management proxy's device
/// among them (`topology::management`).
pub(crate) fn management_tree(device: &DeviceAccess) -> ManagementTree {
    ManagementTree::of(
        device.topology(),
        CONFIG.management_proxy.as_ref().map(|proxy| proxy.device.as_ref()),
    )
}

/// Who asked, for NetBox' changelog: the name or email of the login.
fn user_label(ctx: &Context<'_>) -> Box<str> {
    ctx.data_opt::<UserInfo>()
        .and_then(|user| user.name.clone().or_else(|| user.email.clone()))
        .unwrap_or_else(|| "unknown user".into())
        .into()
}

#[Object]
impl MutationAuthenticated {
    /// Writes `helper`'s plan to NetBox, if it is still the one shown
    /// (`NetboxPlan.fingerprint`). The job's `NetboxWritten` events tell what it wrote;
    /// `undoNetboxWrite` takes that back. The topology is fetched again afterwards.
    #[graphql(guard = "NetboxWritesAllowed")]
    async fn write_netbox_plan(
        &self,
        ctx: &Context<'_>,
        helper: Helper,
        fingerprint: String,
    ) -> Result<Arc<Job>, ApiError> {
        let current = self.topology.current().await.ok_or(Error::NoTopology)?;
        let device = helper.device(&current)?;
        let user = user_label(ctx);
        let topology = self.topology.clone();
        Ok(job::start(&device, JobKind::NetboxWrite, |log| async move {
            let result = helper::write(&helper, &current, &fingerprint, &user, &log).await;
            log.enter(NetboxState::Refetching);
            refetch(&topology).await;
            result
        })?)
    }
    /// Takes back what the `NetboxWrite` job `job_id` wrote, the last first.
    #[graphql(guard = "NetboxWritesAllowed")]
    async fn undo_netbox_write(&self, ctx: &Context<'_>, job_id: u32) -> Result<Arc<Job>, ApiError> {
        let written = job::get(job_id)
            .filter(|job| job.job_kind() == JobKind::NetboxWrite && !job.is_running())
            .ok_or(Error::UnknownJob(job_id))?;
        let objects = written.netbox_written();
        if objects.is_empty() {
            return Err(Error::NothingToUndo(job_id).into());
        }
        let device = self
            .device(written.job_device().ok_or(Error::UnknownJob(job_id))?.0)
            .await?;
        let user = user_label(ctx);
        let topology = self.topology.clone();
        Ok(job::start(&device, JobKind::NetboxUndo, |log| async move {
            let result = helper::undo(job_id, &objects, &user, &log).await;
            log.enter(NetboxState::Refetching);
            refetch(&topology).await;
            result
        })?)
    }
    /// Starts the initial setup over mac-telnet from a neighbor along
    /// `DeviceAccess.bootstrapPlan`, for a device that can't be reached over IP yet
    /// (`allow_apply` must be set). `mac` is the physical device that gets this NetBox
    /// device's config, usually one of `bootstrapPlan.discovered`. The neighbor runs no job of
    /// its own meanwhile (`BUSY` either way): an apply or a reboot of it would cut the session,
    /// and its commands would land on the neighbor's console.
    #[graphql(guard = "WritesAllowed")]
    async fn bootstrap(&self, id: u32, mac: ScalarMac) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let mut plan = bootstrap::plan(&device)?;
        plan.setup.macs = [mac.0].into();
        let neighbor = plan.neighbor_device().clone();
        Ok(job::start_through(&device.clone(), &neighbor, JobKind::Bootstrap, |log| async move {
            bootstrap::bootstrap(&device, &plan, &log).await
        })?)
    }
    /// Starts updating RouterOS (and the RouterBOARD firmware) to the newest release of
    /// `channel`, with the packages the device needs (`allow_apply` must be set); without
    /// `channel` it only brings the packages in line, at the version the device runs. Target
    /// and credentials as for `DeviceAccess.access`. With `suspendWatchdogs` the watchdogs that
    /// may fire while it is away (`DeviceAccess.watchdogs`) are suspended for the update and
    /// restored afterwards. A device without two partitions and without a neighbor to reach it
    /// through (`DeviceAccess.updateNeighbors`) is only updated with `withoutFallback`
    /// (`NO_UPDATE_FALLBACK`). The job's events tell how far it got.
    #[graphql(guard = "WritesAllowed")]
    #[allow(clippy::too_many_arguments)]
    async fn update_routeros(
        &self,
        id: u32,
        channel: Option<update::Channel>,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
        suspend_watchdogs: Option<bool>,
        without_fallback: Option<bool>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        let way_back = update_fallback::WayBack::of(&device, without_fallback == Some(true));
        let watchers = Watchers::of(&device, suspend_watchdogs == Some(true));
        Ok(job::start(&device.clone(), JobKind::Update, |log| async move {
            log.enter(UpdateState::Reading);
            let device = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            let wanted = device.wanted_packages().await?;
            let version = match channel {
                Some(channel) => update::newest(7, channel).await?,
                None => update::read_state(&device.api().await?).await?.version,
            };
            log.emit(update::UpdateTarget {
                version: version.to_string().into(),
                channel: channel.map(|channel| channel.name().into()),
            });
            let suspended = watchers.suspend(&log).await?;
            let updated =
                update::update(&device, &power, &version, &wanted, CONFIG.update_fallback(), &way_back, &log)
                    .await;
            restore_watchdogs(suspended, &log).await;
            updated
        })?)
    }
    /// Starts taking RouterOS 7 on device `id` back to the older `version` (`allow_apply` must
    /// be set): the packages it runs, of that version, then `/system/package/downgrade`. The
    /// rest as for `updateRouteros`, without a channel.
    #[graphql(guard = "WritesAllowed")]
    #[allow(clippy::too_many_arguments)]
    async fn downgrade_routeros(
        &self,
        id: u32,
        version: String,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
        suspend_watchdogs: Option<bool>,
        without_fallback: Option<bool>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let version = update::Version::parse(&version).ok_or_else(|| Error::NoVersion(version.into()))?;
        let power = PowerChain::of(&device);
        let way_back = update_fallback::WayBack::of(&device, without_fallback == Some(true));
        let watchers = Watchers::of(&device, suspend_watchdogs == Some(true));
        Ok(job::start(&device.clone(), JobKind::Downgrade, |log| async move {
            log.enter(UpdateState::Reading);
            let device = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            let suspended = watchers.suspend(&log).await?;
            let downgraded = update::downgrade(&device, &power, &version, &way_back, &log).await;
            restore_watchdogs(suspended, &log).await;
            downgraded
        })?)
    }
    /// Starts switching off and on the PoE of the device feeding device `id`
    /// (`DeviceAccess.powerSupply`; `allow_apply` must be set): a cold boot, which confirms a
    /// device mode change. A job of the supplier, so it runs next to one of the device (the
    /// device mode waiting for its confirmation); it waits while the device or one it powers
    /// updates, and keeps them from updating until the device answers again.
    #[graphql(guard = "WritesAllowed")]
    async fn power_cycle(&self, id: u32) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let (supplier, supply, factory) = crate::device::power_cycle::supply(&device)
            .ok_or_else(|| Error::NoPowerSupply(device.name().into()))?;
        let power = PowerChain::of(&device);
        let name: Box<str> = device.name().into();
        Ok(job::start(&supplier.clone(), JobKind::PowerCycle, |log| async move {
            log.enter(PowerCycleState::Reading);
            let link = connect_for_job(&supplier, None, None, None, &log).await?;
            let cut =
                crate::device::power_cycle::power_cycle(&link.api().await?, &supply, &factory, &power, &name, &log)
                    .await?;
            let back = crate::device::power_cycle::wait_for_fed(&device, &log).await;
            drop(cut);
            back
        })?)
    }
    /// Starts taking care of something a job left on device `id` (`AccessibleDevice.leftovers`;
    /// `allow_apply` must be set). Target and credentials as for `DeviceAccess.access`.
    /// Watchdogs a running job of another device holds suspended aren't restored
    /// (`LEFTOVER_HELD`): that job does it at its end.
    #[graphql(guard = "WritesAllowed")]
    async fn clear_leftover(
        &self,
        id: u32,
        action: crate::device::leftovers::LeftoverAction,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let name: Box<str> = device.name().into();
        let power = PowerChain::of(&device);
        if action == crate::device::leftovers::LeftoverAction::RestoreWatchdogs
            && let Some(held) = job::holding_watchdogs(&name)
        {
            return Err(Error::LeftoverHeld { device: name, job: held.job }.into());
        }
        Ok(job::start(&device.clone(), JobKind::Cleanup, |log| async move {
            let link = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            crate::device::leftovers::clear(&link, &name, action, &power, &log).await
        })?)
    }
    /// Starts a reboot of a device (`allow_apply` must be set) and waits until it is back.
    /// Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn reboot_device(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        Ok(job::start(&device.clone(), JobKind::Reboot, |log| async move {
            log.enter(RebootState::Reading);
            let device = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            update::reboot(&device, &power, &log).await
        })?)
    }
    /// Starts turning on the features of the device mode the provisioner needs
    /// (`DeviceAccess.access.deviceMode.changes`; `allow_apply` must be set). The device asks for
    /// a confirmation at the device (`DeviceModeConfirm`): its button, or cutting its power.
    /// Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn change_device_mode(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        Ok(job::start(&device.clone(), JobKind::DeviceMode, |log| async move {
            log.enter(DeviceModeState::Reading);
            let link = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            device_mode::change(&link, &device, &power, &log).await
        })?)
    }
    /// Starts splitting the flash in two partitions (`DeviceAccess.access.partitions.canSplit`;
    /// `allow_apply` must be set): the device reformats its flash and reboots. Target and
    /// credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn repartition_device(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        Ok(job::start(&device.clone(), JobKind::Repartition, |log| async move {
            log.enter(RebootState::Reading);
            let link = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            partitions::repartition(&link, &power, &log).await
        })?)
    }
    /// Starts booting the device from `partition` (one of `DeviceAccess.access.partitions` with
    /// a RouterOS on it; `allow_apply` must be set), e.g. back to the RouterOS from before an
    /// update. Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn boot_partition(
        &self,
        id: u32,
        partition: Box<str>,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        Ok(job::start(&device.clone(), JobKind::BootPartition, |log| async move {
            log.enter(RebootState::Reading);
            let link = connect_for_job(&device, target, credential_name, adhoc_credentials, &log).await?;
            partitions::boot_from(&link, &power, &partition, &log).await
        })?)
    }
    /// Stops a job (`Query.job`) at the next point where that is safe: an update between its
    /// steps or while it waits, removing the packages it uploaded unless the device reboots
    /// with them already; an apply before its script runs, or after, leaving the fallback
    /// that brings back the config from before; a setup before or after its console session; a
    /// reboot before it is sent, or while it is waited for.
    async fn cancel_job(&self, id: u32) -> Result<Arc<Job>, ApiError> {
        Ok(job::cancel(id).ok_or(Error::UnknownJob(id))?)
    }
    /// Removes the file of a job saved before the restart that didn't go on (`savedJobs`);
    /// what it left on the device stays, `leftovers` finds it.
    #[graphql(guard = "WritesAllowed")]
    async fn discard_saved_job(&self, device_id: u32) -> Result<bool, ApiError> {
        match crate::saved_job::discard(DeviceId(device_id)) {
            true => Ok(true),
            false => Err(Error::NoSavedJob(device_id).into()),
        }
    }
    /// Starts applying the generated config to `ids` together (`allow_apply` must be set),
    /// each with its NetBox address and credentials: one job per device, with one fallback
    /// for all of them, taken away only once every one is reached again (`device::batch`).
    #[graphql(guard = "WritesAllowed")]
    /// Starts reading how far each RouterOS device of the site is from NetBox (what an apply
    /// would change, by menu; `device::site_diff`), or gives back the one still running. Only
    /// reads, so it needs no `allow_apply`.
    async fn diff_site(&self, site_id: u32) -> Result<Arc<Job>, ApiError> {
        let site = self.site(site_id).await?;
        Ok(job::start_site(&site.clone(), JobKind::SiteDiff, |log| {
            crate::device::site_diff::run(site, log)
        }))
    }
    async fn apply_batch(&self, ids: Vec<u32>) -> Result<Vec<Arc<Job>>, ApiError> {
        let mut devices = Vec::new();
        for id in ids {
            let device = self.device(id).await?;
            if !devices.iter().any(|d: &DeviceAccess| d.id() == device.id()) {
                devices.push(device);
            }
        }
        let members = devices.clone();
        Ok(job::start_all(&devices, JobKind::Apply, |logs| {
            crate::device::batch::apply(members, logs)
        })?)
    }
    /// Starts applying the generated config to a device (`allow_apply` must be set). Target
    /// and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn apply_cfg(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        Ok(job::start(&device.clone(), JobKind::Apply, |log| async move {
            connect_for_job(&device, target, credential_name, adhoc_credentials, &log)
                .await?
                .apply(&log)
                .await
        })?)
    }
}
#[Object]
impl QueryAnonymous {
    /// gives the coordinates for authentication
    async fn authentication(&self) -> AuthenticationData {
        AuthenticationData {
            client_id: CONFIG.auth_client_id(),
            issuer_url: CONFIG.auth_issuer(),
            scopes: CONFIG.auth_scopes(),
        }
    }
}
#[derive(SimpleObject)]
struct AuthenticationData {
    client_id: &'static str,
    issuer_url: &'static str,
    scopes: Box<[Box<str>]>,
}
