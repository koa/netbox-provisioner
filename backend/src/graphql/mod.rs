use crate::{
    Error,
    config::CONFIG,
    device::{
        bootstrap,
        power::PowerChain,
        update::{self, DeviceLink},
    },
    graphql::scalars::ScalarMac,
    job::{self, Job, JobKind},
    topology::{DeviceId, TopologyHolder, access::AdhocCredentials},
};
use std::sync::Arc;
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
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
    async fn job(&self, id: u32) -> Option<Arc<Job>> {
        job::get(id)
    }
    /// The jobs of a device (of all devices without `device_id`), newest first; those still
    /// running and the last finished ones.
    async fn jobs(&self, device_id: Option<u32>) -> Vec<Arc<Job>> {
        job::list(device_id.map(DeviceId))
    }
}
#[Object]
impl MutationAuthenticated {
    /// Starts the initial setup over mac-telnet from a neighbor along
    /// `DeviceAccess.bootstrapPlan`, for a device that can't be reached over IP yet
    /// (`allow_apply` must be set). `mac` is the physical device that gets this NetBox
    /// device's config, usually one of `bootstrapPlan.discovered`.
    async fn bootstrap(&self, id: u32, mac: ScalarMac) -> Result<Arc<Job>, Error> {
        if !CONFIG.allow_apply {
            return Err(Error::ApplyNotAllowed);
        }
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        let mut plan = bootstrap::plan(&device)?;
        plan.setup.macs = [mac.0].into();
        job::start(&device.clone(), JobKind::Bootstrap, |log| async move {
            bootstrap::bootstrap(&device, &plan, &log).await
        })
    }
    /// Starts updating RouterOS (and the RouterBOARD firmware) to the newest release of
    /// `channel`, with the packages the device needs (`allow_apply` must be set); without
    /// `channel` it only brings the packages in line, at the version the device runs. Target
    /// and credentials as for `DeviceAccess.access`. The job's events tell how far it got.
    async fn update_routeros(
        &self,
        id: u32,
        channel: Option<update::Channel>,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, Error> {
        if !CONFIG.allow_apply {
            return Err(Error::ApplyNotAllowed);
        }
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        let power = PowerChain::of(&device);
        job::start(&device.clone(), JobKind::Update, |log| async move {
            let device = log
                .unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?;
            let wanted = device.wanted_packages().await?;
            let version = match channel {
                Some(channel) => update::newest(7, channel).await?,
                None => update::read_state(&device.api().await?).await?.version,
            };
            update::update(&device, &power, &version, &wanted, &log).await
        })
    }
    /// Stops a job (`Query.job`) at the next point where that is safe: an update between its
    /// steps or while it waits, removing the packages it uploaded unless the device reboots
    /// with them already; an apply before its script runs, or after, leaving the fallback
    /// that brings back the config from before; a setup before or after its console session.
    async fn cancel_job(&self, id: u32) -> Result<Arc<Job>, Error> {
        job::cancel(id).ok_or(Error::UnknownJob(id))
    }
    /// Starts applying the generated config to a device (`allow_apply` must be set). Target
    /// and credentials as for `DeviceAccess.access`.
    async fn apply_cfg(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<Arc<Job>, Error> {
        if !CONFIG.allow_apply {
            return Err(Error::ApplyNotAllowed);
        }
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        job::start(&device.clone(), JobKind::Apply, |log| async move {
            log.unless_cancelled(device.connect(target, credential_name, adhoc_credentials))
                .await??
                .ok_or(Error::MissingCredentials)?
                .apply(&log)
                .await
        })
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
