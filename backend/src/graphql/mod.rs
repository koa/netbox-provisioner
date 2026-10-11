use crate::{
    Error,
    config::CONFIG,
    device::{
        bootstrap::{self, BootstrapResult},
        update::{self, UpdateResult},
        ros::graphql::ApplyResult,
    },
    topology::{DeviceId, TopologyHolder, access::AdhocCredentials},
};
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
}
#[Object]
impl MutationAuthenticated {
    /// Initial setup over mac-telnet from a neighbor along `DeviceAccess.bootstrapPlan`, for a
    /// device that can't be reached over IP yet (`allow_apply` must be set). `mac` is the
    /// physical device that gets this NetBox device's config, usually one of
    /// `bootstrapPlan.discovered`.
    async fn bootstrap(&self, id: u32, mac: String) -> Result<BootstrapResult, Error> {
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        let mut plan = bootstrap::plan(&device)?;
        let mac = bootstrap::parse_mac(&mac)
            .ok_or_else(|| Error::Bootstrap(device.name().into(), format!("not a MAC address: {mac}").into()))?;
        plan.setup.macs = vec![mac];
        Ok(bootstrap::bootstrap(&device, &plan).await)
    }
    /// Updates RouterOS (and the RouterBOARD firmware) to the newest release of `channel`, with
    /// the packages the device needs (`allow_apply` must be set). Target and credentials as for
    /// `DeviceAccess.access`.
    async fn update_routeros(
        &self,
        id: u32,
        channel: update::Channel,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<UpdateResult, Error> {
        if !CONFIG.allow_apply {
            return Err(Error::ApplyNotAllowed);
        }
        let version = update::newest(7, channel).await?;
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        let device = device
            .connect(target, credential_name, adhoc_credentials)
            .await?
            .ok_or(Error::MissingCredentials)?;
        let wanted = device.wanted_packages().await?;
        Ok(update::update(&device, &version, &wanted).await)
    }
    /// Applies the generated config to a device (`allow_apply` must be set). Target and
    /// credentials as for `DeviceAccess.access`.
    async fn apply_cfg(
        &self,
        id: u32,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<ApplyResult, Error> {
        let device = self
            .topology
            .devices_by_id(DeviceId(id))
            .await
            .ok_or(Error::UnknownDevice(id))?;
        device
            .connect(target, credential_name, adhoc_credentials)
            .await?
            .ok_or(Error::MissingCredentials)?
            .apply()
            .await
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
