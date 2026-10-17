use crate::{
    Error,
    config::CONFIG,
    error_info::ApiError,
    device::{
        bootstrap, device_mode, partitions,
        power::{self, PowerChain},
        update::{self, DeviceLink},
        watchdog,
    },
    context::UserInfo,
    graphql::scalars::ScalarMac,
    helper::{self, Helper, NetboxPlan},
    job::{self, Job, JobKind},
    netbox,
    topology::{
        DeviceId, TopologyHolder,
        access::{AdhocCredentials, device::DeviceAccess},
    },
};
use std::sync::Arc;
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
    async fn job(&self, id: u32) -> Option<Arc<Job>> {
        job::get(id)
    }
    /// The jobs of a device (of all devices without `device_id`), newest first; those still
    /// running and the last finished ones.
    async fn jobs(&self, device_id: Option<u32>) -> Vec<Arc<Job>> {
        job::list(device_id.map(DeviceId))
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

/// The watchdog of `device` couldn't be suspended for this.
fn not_suspended(device: &str) -> impl Fn(Error) -> Error + '_ {
    move |error| Error::WatchdogNotSuspended {
        device: device.into(),
        cause: error.to_string().into(),
    }
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
        let device = self.device(written.job_device().0).await?;
        let user = user_label(ctx);
        let topology = self.topology.clone();
        Ok(job::start(&device, JobKind::NetboxUndo, |log| async move {
            let result = helper::undo(job_id, &objects, &user, &log).await;
            refetch(&topology).await;
            result
        })?)
    }
    /// Starts the initial setup over mac-telnet from a neighbor along
    /// `DeviceAccess.bootstrapPlan`, for a device that can't be reached over IP yet
    /// (`allow_apply` must be set). `mac` is the physical device that gets this NetBox
    /// device's config, usually one of `bootstrapPlan.discovered`.
    #[graphql(guard = "WritesAllowed")]
    async fn bootstrap(&self, id: u32, mac: ScalarMac) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let mut plan = bootstrap::plan(&device)?;
        plan.setup.macs = [mac.0].into();
        Ok(job::start(&device.clone(), JobKind::Bootstrap, |log| async move {
            bootstrap::bootstrap(&device, &plan, &log).await
        })?)
    }
    /// Starts updating RouterOS (and the RouterBOARD firmware) to the newest release of
    /// `channel`, with the packages the device needs (`allow_apply` must be set); without
    /// `channel` it only brings the packages in line, at the version the device runs. Target
    /// and credentials as for `DeviceAccess.access`. With `suspendWatchdogs` the watchdogs of the
    /// devices powering it (`DeviceAccess.supplierWatchdogs`) are suspended for the update and
    /// restored afterwards. The job's events tell how far it got.
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
    ) -> Result<Arc<Job>, ApiError> {
        let device = self.device(id).await?;
        let power = PowerChain::of(&device);
        let suppliers: Vec<_> = match suspend_watchdogs {
            Some(true) => power::supplier_devices(&device)
                .into_iter()
                .filter(|supplier| supplier.has_routeros())
                .collect(),
            _ => Vec::new(),
        };
        Ok(job::start(&device.clone(), JobKind::Update, |log| async move {
            let device = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
            let wanted = device.wanted_packages().await?;
            let version = match channel {
                Some(channel) => update::newest(7, channel).await?,
                None => update::read_state(&device.api().await?).await?.version,
            };
            let mut links = Vec::new();
            for supplier in suppliers {
                let name: Box<str> = supplier.name().into();
                let link = match log.unless_cancelled(supplier.connect(None, None, None)).await? {
                    Ok(Some(link)) => link,
                    Ok(None) => Err(Error::MissingCredentials).map_err(not_suspended(&name))?,
                    Err(error) => Err(error).map_err(not_suspended(&name))?,
                };
                links.push((name, link));
            }
            let suspended = watchdog::suspend_all(links, &log).await?;
            let updated =
                update::update(&device, &power, &version, &wanted, CONFIG.update_fallback(), &log).await;
            watchdog::restore_all(suspended, &log).await;
            updated
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
            let device = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
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
            let link = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
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
            let link = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
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
            let link = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
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
            log.unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?
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
