//! Scope: The roots of the GraphQL schema: each query, mutation and subscription field with its
//! guard; not the work behind a field (the module it calls).
use crate::{
    Error,
    config::CONFIG,
    context::UserInfo,
    device::{
        AccessibleDevice, bootstrap, connect_for_job,
        dependency::PowerChain,
        device_mode, partitions, reboot, rollout,
        update::{self, DeviceLink},
        update_fallback,
        watchdog::{Watchers, restore_watchdogs},
    },
    device_state::{
        BootstrapState, DeviceModeState, JobPhase, NetboxState, PowerCycleState, RebootState,
        UpdateState,
    },
    error_info::ApiError,
    graphql::scalars::ScalarMac,
    helper::{self, Helper, NetboxPlan},
    job::{self, Job, JobKind, JobSubject, Log, resume::saved_update},
    journal::Started,
    netbox, saved_job,
    topology::{
        DeviceId, SiteId, TopologyHolder,
        access::{DeviceTarget, device::DeviceAccess, site::SiteAccess},
    },
};
use async_graphql::{
    Context, EmptyMutation, EmptySubscription, Guard, InputObject, Object, Schema, SimpleObject,
    Subscription, futures_util::Stream,
};
use serde_json::json;
use std::sync::Arc;
pub mod live;
pub mod scalars;
pub type AuthenticatedGraphqlSchema =
    Schema<QueryAuthenticated, MutationAuthenticated, SubscriptionAuthenticated>;
pub type AnonymousGraphqlSchema = Schema<QueryAnonymous, EmptyMutation, EmptySubscription>;

pub struct QueryAuthenticated {
    topology: TopologyHolder,
}
pub struct QueryAnonymous;
/// The fields of [`QueryAuthenticated`] worth watching, with the same types: each gives its
/// value again whenever what the selection under it read changed (`live`).
pub struct SubscriptionAuthenticated {
    topology: TopologyHolder,
}
pub struct MutationAuthenticated {
    topology: TopologyHolder,
}

pub fn create_schema(topology: TopologyHolder) -> AuthenticatedGraphqlSchema {
    Schema::build(
        QueryAuthenticated {
            topology: topology.clone(),
        },
        MutationAuthenticated {
            topology: topology.clone(),
        },
        SubscriptionAuthenticated { topology },
    )
    .extension(live::LiveExtension)
    .finish()
}
pub fn create_anonymous_schema() -> AnonymousGraphqlSchema {
    Schema::build(QueryAnonymous, EmptyMutation, EmptySubscription).finish()
}

#[Object]
impl QueryAuthenticated {
    async fn topology(&self) -> TopologyHolder {
        self.topology.snapshot().await
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
        job::list_shown(job_subject(device_id, site_id))
    }
    /// What a rollout over the sites `siteIds` would do, to the newest release of `channel`
    /// (`routeros_channel` of the config without): every RouterOS device of them read, in the
    /// order it would update, those left out last (`device::rollout`). Only reads.
    async fn rollout_plan(
        &self,
        site_ids: Vec<u32>,
        channel: Option<update::Channel>,
    ) -> Result<crate::device::rollout::RolloutPlan, ApiError> {
        let topology = self.topology.snapshot().await;
        let mut sites = Vec::new();
        for id in site_ids {
            let site = topology.site(SiteId(id)).await;
            sites.push(site.ok_or(Error::UnknownSite(id))?);
        }
        let channel = channel.or(CONFIG.routeros_channel).unwrap_or_default();
        Ok(rollout::plan(&sites, channel).await?)
    }
    /// The newest rollout job (`startRollout`), running or not.
    async fn rollout_job(&self) -> Option<Arc<Job>> {
        job::newest_of_network(JobKind::Rollout)
    }
    /// The jobs saved before the restart that didn't go on: their device is gone from NetBox,
    /// or the file couldn't be read. They stay until discarded (`discardSavedJob`).
    async fn saved_jobs(&self) -> Vec<crate::saved_job::Waiting> {
        crate::saved_job::waiting()
    }
}
fn job_subject(device_id: Option<u32>, site_id: Option<u32>) -> Option<JobSubject> {
    device_id
        .map(|id| JobSubject::Device(DeviceId(id)))
        .or(site_id.map(|id| JobSubject::Site(SiteId(id))))
}

#[Subscription]
impl SubscriptionAuthenticated {
    /// `topology` of the query, again whenever what was read under it changed.
    async fn topology(&self) -> impl Stream<Item = TopologyHolder> + use<> {
        let topology = self.topology.clone();
        live::rounds(move || {
            let topology = topology.clone();
            async move { topology.snapshot().await }
        })
    }
    /// `job` of the query, again whenever it logs or ends, its `events` then only the new ones;
    /// `null` once the server doesn't know it (it restarted). A site's job tells nobody: one
    /// answer only.
    async fn job(&self, id: u32) -> impl Stream<Item = Option<Arc<Job>>> + use<> {
        live::rounds(move || {
            let job = job::get(id);
            for device in job.iter().flat_map(|job| job.log().devices()) {
                crate::facts::note_jobs(Some(*device));
            }
            std::future::ready(job)
        })
    }
    /// `jobs` of the query, again whenever one of them started, logged or ended.
    async fn jobs(
        &self,
        device_id: Option<u32>,
        site_id: Option<u32>,
    ) -> impl Stream<Item = Vec<Arc<Job>>> + use<> {
        let subject = job_subject(device_id, site_id);
        live::rounds(move || std::future::ready(job::list_shown(subject)))
    }
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
        self.topology
            .site(SiteId(id))
            .await
            .ok_or(Error::UnknownSite(id))
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
/// `netbox_write_token`. Who may use which helper isn't told apart yet (`docs/config-helper.md`,
/// the roles of `docs/concepts/self-service.md`): every user of `auth_users` may.
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

/// Starts a job of `kind` on `device` for the user of `ctx`: it enters `state` (where its kind
/// begins with one), connects through `target` (`connect_for_job`) and does `work` with the
/// connection, the device, its PoE chain and the log.
fn start_device_job<F, W>(
    ctx: &Context<'_>,
    device: DeviceAccess,
    kind: JobKind,
    state: Option<JobPhase>,
    target: Option<DeviceTarget>,
    work: W,
) -> Result<Arc<Job>, ApiError>
where
    W: FnOnce(AccessibleDevice, DeviceAccess, PowerChain, Log) -> F + Send + 'static,
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let started = Started::By(user_label(ctx));
    Ok(start_device_job_as(
        started, device, kind, state, target, work,
    )?)
}

/// [`start_device_job`] for whoever `started` names (a rollout's user).
fn start_device_job_as<F, W>(
    started: Started,
    device: DeviceAccess,
    kind: JobKind,
    state: Option<JobPhase>,
    target: Option<DeviceTarget>,
    work: W,
) -> Result<Arc<Job>, Error>
where
    W: FnOnce(AccessibleDevice, DeviceAccess, PowerChain, Log) -> F + Send + 'static,
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    job::start(&device.clone(), kind, started, |log| async move {
        if let Some(state) = state {
            log.enter(state);
        }
        let power = PowerChain::of(&device).await;
        let link = connect_for_job(&device, target.unwrap_or_default(), &log).await?;
        work(link, device, power, log).await
    })
}

/// The job of `updateRouteros`, for whoever `started` names.
fn start_update(
    started: Started,
    device: DeviceAccess,
    channel: Option<update::Channel>,
    target: Option<DeviceTarget>,
    suspend_watchdogs: bool,
    without_fallback: bool,
) -> Result<Arc<Job>, Error> {
    let way_back = update_fallback::WayBack::of(&device, without_fallback);
    let watchers = Watchers::of(&device, suspend_watchdogs);
    start_device_job_as(
        started,
        device,
        JobKind::Update,
        Some(UpdateState::Reading.into()),
        target,
        move |device, access, power, log| async move {
            let wanted = device.wanted_packages().await?;
            let version = match channel {
                Some(channel) => update::newest(7, channel).await?,
                None => update::read_state(&device.api().await?).await?.version,
            };
            log.emit(update::UpdateTarget {
                version: version.to_string().into(),
                channel: channel.map(|channel| channel.name().into()),
            });
            let suspended = watchers
                .suspend(&log, UpdateState::SuspendingWatchdogs)
                .await?;
            let watching: Vec<_> = suspended
                .iter()
                .map(|s| (s.device.clone(), s.watching.clone()))
                .collect();
            let credentials = device.credentials_name().map(Box::from);
            // without credentials by name (ad hoc ones) it can't connect again after a restart
            let keep = |run: &saved_job::UpdateRun| {
                if let Some(credentials) = &credentials {
                    log.keep(saved_update(&access, credentials, &watching, run));
                }
            };
            let fallback = CONFIG.update_fallback();
            let updated = update::update_keeping(
                &device, &power, &version, &wanted, fallback, &way_back, &keep, &log,
            )
            .await;
            restore_watchdogs(suspended, &log, UpdateState::RestoringWatchdogs).await;
            updated
        },
    )
}

/// Who asked, for NetBox' changelog: the name or email of the login.
fn user_label(ctx: &Context<'_>) -> Box<str> {
    ctx.data_opt::<UserInfo>()
        .and_then(|user| user.name.clone().or_else(|| user.email.clone()))
        .unwrap_or_else(|| "unknown user".into())
        .into()
}

/// One device's position for `MutationAuthenticated.publishLayout`.
#[derive(InputObject)]
struct DevicePositionInput {
    device_id: u32,
    x: i32,
    y: i32,
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
        Ok(job::start(
            &device,
            JobKind::NetboxWrite,
            Started::By(user_label(ctx)),
            |log| async move {
                let result = helper::write(&helper, &current, &fingerprint, &user, &log).await;
                log.enter(NetboxState::Refetching);
                refetch(&topology).await;
                result
            },
        )?)
    }
    /// Takes back what the `NetboxWrite` job `job_id` wrote, the last first.
    #[graphql(guard = "NetboxWritesAllowed")]
    async fn undo_netbox_write(
        &self,
        ctx: &Context<'_>,
        job_id: u32,
    ) -> Result<Arc<Job>, ApiError> {
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
        Ok(job::start(
            &device,
            JobKind::NetboxUndo,
            Started::By(user_label(ctx)),
            |log| async move {
                let result = helper::undo(job_id, &objects, &user, &log).await;
                log.enter(NetboxState::Refetching);
                refetch(&topology).await;
                result
            },
        )?)
    }
    /// Publishes `positions` as the network map's official layout, custom field `map_position`
    /// of each device (`docs/concepts/network-map.md`). One write per device, not a `NetboxPlan`:
    /// a position is a single idempotent field with nothing else depending on it, so none of
    /// that machinery's fingerprint check, undo log or job buys anything here. Not a job itself
    /// either, since a handful of fast field writes don't need one; the topology is fetched
    /// again before returning so the next `networkState` already has them.
    #[graphql(guard = "NetboxWritesAllowed")]
    async fn publish_layout(
        &self,
        ctx: &Context<'_>,
        positions: Vec<DevicePositionInput>,
    ) -> Result<u32, ApiError> {
        let rest = netbox::rest::NetboxRest::writer(format!(
            "Netz-Karte, Layout publiziert ({})",
            user_label(ctx)
        ))
        .map_err(Error::Netbox)?;
        for position in &positions {
            rest.update(
                &format!("dcim/devices/{}/", position.device_id),
                json!({ "custom_fields": { "map_position": { "x": position.x, "y": position.y } } }),
            )
            .await
            .map_err(Error::Netbox)?;
        }
        refetch(&self.topology).await;
        Ok(positions.len() as u32)
    }
    /// Starts the initial setup over mac-telnet from a neighbor along
    /// `DeviceAccess.bootstrapPlan` (the first neighbor that answers, checked again), for a device that can't be reached over IP yet
    /// (`allow_apply` must be set). `mac` is the physical device that gets this NetBox
    /// device's config, usually one of `bootstrapPlan.discovered`. The neighbor runs no job of
    /// its own meanwhile (`BUSY` either way): an apply or a reboot of it would cut the session,
    /// and its commands would land on the neighbor's console.
    #[graphql(guard = "WritesAllowed")]
    async fn bootstrap(
        &self,
        ctx: &Context<'_>,
        id: u32,
        mac: ScalarMac,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let mut plan = bootstrap::working_plan(&device).await?;
        plan.setup.macs = [mac.0].into();
        let neighbor = plan.neighbor_device().clone();
        let watchers = Watchers::suppliers(&device);
        Ok(job::start_through(
            &device.clone(),
            &neighbor,
            JobKind::Bootstrap,
            Started::By(user_label(ctx)),
            |log| async move {
                let suspended = watchers
                    .suspend(&log, BootstrapState::SuspendingWatchdogs)
                    .await?;
                let set_up = bootstrap::bootstrap(&device, &plan, &log).await;
                restore_watchdogs(suspended, &log, BootstrapState::RestoringWatchdogs).await;
                set_up
            },
        )?)
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
        ctx: &Context<'_>,
        id: u32,
        channel: Option<update::Channel>,
        target: Option<DeviceTarget>,
        suspend_watchdogs: Option<bool>,
        without_fallback: Option<bool>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        Ok(start_update(
            Started::By(user_label(ctx)),
            device,
            channel,
            target,
            suspend_watchdogs == Some(true),
            without_fallback == Some(true),
        )?)
    }
    /// Starts a rollout (`docs/concepts/network-update.md`): the RouterOS devices of the sites
    /// to the newest release of `channel` (the config's without), an update job each, as
    /// `rolloutPlan` plans it when the job starts; `pilots` (device ids) instead of the pilots
    /// it proposes. The watchdogs of the suppliers are suspended for each update. The job pauses
    /// after the pilots and after a device that failed (`RolloutPaused`) until
    /// `continueRollout`; `cancelJob` ends it and cancels the updates it started. One rollout
    /// at a time: while one runs, that one is given back.
    #[graphql(guard = "WritesAllowed")]
    async fn start_rollout(
        &self,
        ctx: &Context<'_>,
        site_ids: Vec<u32>,
        channel: Option<update::Channel>,
        pilots: Option<Vec<u32>>,
    ) -> Result<Arc<Job>, ApiError> {
        let snapshot = self.topology.snapshot().await;
        let mut sites = Vec::new();
        for id in site_ids {
            let site = snapshot.site(SiteId(id)).await;
            sites.push(site.ok_or(Error::UnknownSite(id))?);
        }
        let channel = channel.or(CONFIG.routeros_channel).unwrap_or_default();
        let user = user_label(ctx);
        let topology = self.topology.clone();
        let control = Arc::new(rollout::run::Control::default());
        let running = control.clone();
        let job = job::start_network("rollout", JobKind::Rollout, |log| async move {
            let _downloads = update::DownloadCache::hold();
            let mut plan = log
                .unless_cancelled(rollout::plan(&sites, channel))
                .await??;
            if let Some(pilots) = pilots {
                rollout::choose_pilots(&mut plan, &pilots);
            }
            let downloads = rollout::download_all(&plan, &log);
            rollout::run::run(plan.clone(), &running, &log, downloads, |id| {
                let (topology, user) = (topology.clone(), user.clone());
                async move {
                    let device = topology
                        .devices_by_id(DeviceId(id))
                        .await
                        .ok_or(Error::UnknownDevice(id))?;
                    start_update(Started::By(user), device, Some(channel), None, true, false)
                }
            })
            .await
        });
        rollout::run::Control::register(job.job_id(), control);
        Ok(job)
    }
    /// Lets the paused rollout job `job_id` go on (`RolloutPaused`).
    #[graphql(guard = "WritesAllowed")]
    async fn continue_rollout(&self, job_id: u32) -> Result<bool, ApiError> {
        let control = rollout::run::Control::of(job_id).ok_or(Error::UnknownJob(job_id))?;
        control.go_on();
        Ok(true)
    }
    /// Leaves device `device_id` out of the running rollout job `job_id`, unless its update
    /// started already.
    #[graphql(guard = "WritesAllowed")]
    async fn skip_rollout_device(&self, job_id: u32, device_id: u32) -> Result<bool, ApiError> {
        let control = rollout::run::Control::of(job_id).ok_or(Error::UnknownJob(job_id))?;
        control.skip(device_id);
        Ok(true)
    }
    /// Starts taking RouterOS 7 on device `id` back to the older `version` (`allow_apply` must
    /// be set): the packages it runs, of that version, then `/system/package/downgrade`. The
    /// rest as for `updateRouteros`, without a channel.
    #[graphql(guard = "WritesAllowed")]
    #[allow(clippy::too_many_arguments)]
    async fn downgrade_routeros(
        &self,
        ctx: &Context<'_>,
        id: u32,
        version: String,
        target: Option<DeviceTarget>,
        suspend_watchdogs: Option<bool>,
        without_fallback: Option<bool>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let version =
            update::Version::parse(&version).ok_or_else(|| Error::NoVersion(version.into()))?;
        let way_back = update_fallback::WayBack::of(&device, without_fallback == Some(true));
        let watchers = Watchers::of(&device, suspend_watchdogs == Some(true));
        start_device_job(
            ctx,
            device,
            JobKind::Downgrade,
            Some(UpdateState::Reading.into()),
            target,
            move |device, _, power, log| async move {
                let suspended = watchers
                    .suspend(&log, UpdateState::SuspendingWatchdogs)
                    .await?;
                let downgraded =
                    update::downgrade(&device, &power, &version, &way_back, &log).await;
                restore_watchdogs(suspended, &log, UpdateState::RestoringWatchdogs).await;
                downgraded
            },
        )
    }
    /// Starts switching off and on the PoE of the device feeding device `id`
    /// (`DeviceAccess.powerSupply`; `allow_apply` must be set): a cold boot, which confirms a
    /// device mode change. A job of the supplier, so it runs next to one of the device (the
    /// device mode waiting for its confirmation); it waits while the device or one it powers
    /// updates, and keeps them from updating until the device answers again.
    #[graphql(guard = "WritesAllowed")]
    async fn power_cycle(&self, ctx: &Context<'_>, id: u32) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let crate::device::power_cycle::Supplier {
            device: supplier,
            supply,
            factory,
        } = crate::device::power_cycle::supply(&device)
            .ok_or_else(|| Error::NoPowerSupply(device.name().into()))?;
        let name: Box<str> = device.name().into();
        Ok(job::start(
            &supplier.clone(),
            JobKind::PowerCycle,
            Started::By(user_label(ctx)),
            |log| async move {
                log.enter(PowerCycleState::Reading);
                let power = PowerChain::of(&device).await;
                let link = connect_for_job(&supplier, DeviceTarget::default(), &log).await?;
                let cut = crate::device::power_cycle::power_cycle(
                    &link.api().await?,
                    &supply,
                    &factory,
                    &power,
                    &name,
                    &log,
                )
                .await?;
                let back = crate::device::power_cycle::wait_for_fed(&device, &log).await;
                drop(cut);
                back
            },
        )?)
    }
    /// Starts taking care of something a job left on device `id` (`AccessibleDevice.leftovers`;
    /// `allow_apply` must be set). Target and credentials as for `DeviceAccess.access`.
    /// Watchdogs a running job of another device holds suspended aren't restored
    /// (`LEFTOVER_HELD`): that job does it at its end.
    #[graphql(guard = "WritesAllowed")]
    async fn clear_leftover(
        &self,
        ctx: &Context<'_>,
        id: u32,
        action: crate::device::leftovers::LeftoverAction,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let name: Box<str> = device.name().into();
        if action == crate::device::leftovers::LeftoverAction::RestoreWatchdogs
            && let Some(held) = job::holding_watchdogs(&name)
        {
            return Err(Error::LeftoverHeld {
                device: name,
                job: held.job,
            }
            .into());
        }
        start_device_job(
            ctx,
            device,
            JobKind::Cleanup,
            None,
            target,
            move |link, _, power, log| async move {
                crate::device::leftovers::clear(&link, &name, action, &power, &log).await
            },
        )
    }
    /// Starts a reboot of a device (`allow_apply` must be set) and waits until it is back.
    /// Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn reboot_device(
        &self,
        ctx: &Context<'_>,
        id: u32,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        start_device_job(
            ctx,
            self.device(id).await?,
            JobKind::Reboot,
            Some(RebootState::Reading.into()),
            target,
            |link, _, power, log| async move { reboot::reboot(&link, &power, &log).await },
        )
    }
    /// Starts turning on the features of the device mode the provisioner needs
    /// (`DeviceAccess.access.deviceMode.changes`; `allow_apply` must be set). The device asks for
    /// a confirmation at the device (`DeviceModeConfirm`): its button, or cutting its power.
    /// Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn change_device_mode(
        &self,
        ctx: &Context<'_>,
        id: u32,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        start_device_job(
            ctx,
            self.device(id).await?,
            JobKind::DeviceMode,
            Some(DeviceModeState::Reading.into()),
            target,
            |link, device, power, log| async move {
                device_mode::change(&link, &device, &power, &log).await
            },
        )
    }
    /// Starts splitting the flash in two partitions (`DeviceAccess.access.partitions.canSplit`;
    /// `allow_apply` must be set): the device reformats its flash and reboots. Target and
    /// credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn repartition_device(
        &self,
        ctx: &Context<'_>,
        id: u32,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        start_device_job(
            ctx,
            self.device(id).await?,
            JobKind::Repartition,
            Some(RebootState::Reading.into()),
            target,
            |link, _, power, log| async move { partitions::repartition(&link, &power, &log).await },
        )
    }
    /// Starts booting the device from `partition` (one of `DeviceAccess.access.partitions` with
    /// a RouterOS on it; `allow_apply` must be set), e.g. back to the RouterOS from before an
    /// update. Target and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn boot_partition(
        &self,
        ctx: &Context<'_>,
        id: u32,
        partition: Box<str>,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        start_device_job(
            ctx,
            self.device(id).await?,
            JobKind::BootPartition,
            Some(RebootState::Reading.into()),
            target,
            move |link, _, power, log| async move {
                partitions::boot_from(&link, &power, &partition, &log).await
            },
        )
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
    async fn apply_batch(
        &self,
        ctx: &Context<'_>,
        ids: Vec<u32>,
    ) -> Result<Vec<Arc<Job>>, ApiError> {
        let topology = self.topology.snapshot().await;
        let mut devices = Vec::new();
        for id in ids {
            let device = topology
                .devices_by_id(DeviceId(id))
                .await
                .ok_or(Error::UnknownDevice(id))?;
            if !devices.iter().any(|d: &DeviceAccess| d.id() == device.id()) {
                devices.push(device);
            }
        }
        let members = devices.clone();
        let user = user_label(ctx);
        Ok(job::start_all(
            &devices,
            JobKind::Apply,
            |_| Started::By(user.clone()),
            |logs| crate::device::batch::apply(members, logs),
        )?)
    }
    /// Drops the live checks of a device's interfaces: who shows them reads them anew, a
    /// subscription right away („Erneut prüfen“).
    async fn refresh_interface_live(&self, id: u32) -> bool {
        crate::facts::invalidate(DeviceId(id), &[crate::facts::FactKind::InterfaceLive]);
        true
    }
    /// Drops the neighbors the devices were seen to see (`neighborFindings`): who shows them
    /// reads them anew, a subscription right away („Nochmals prüfen“).
    async fn refresh_neighbors(&self, ids: Vec<u32>) -> bool {
        for id in ids {
            crate::facts::invalidate(DeviceId(id), &[crate::facts::FactKind::Neighbors]);
        }
        true
    }
    /// Drops what was asked of a device (`facts`): who shows it asks anew, a subscription
    /// right away („Neu laden“).
    async fn refresh_device(&self, id: u32) -> bool {
        crate::facts::invalidate(DeviceId(id), crate::facts::FactKind::ABOUT_DEVICES);
        true
    }
    /// Starts applying the generated config to a device (`allow_apply` must be set). Target
    /// and credentials as for `DeviceAccess.access`.
    #[graphql(guard = "WritesAllowed")]
    async fn apply_cfg(
        &self,
        ctx: &Context<'_>,
        id: u32,
        target: Option<DeviceTarget>,
    ) -> Result<Arc<Job>, ApiError> {
        start_device_job(
            ctx,
            self.device(id).await?,
            JobKind::Apply,
            None,
            target,
            |link, _, _, log| async move { link.apply(&log).await },
        )
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
