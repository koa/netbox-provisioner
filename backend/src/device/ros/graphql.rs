use crate::device::{AccessibleDevice, Credentials, PingResult};
use crate::{
    Error,
    device::{
        GraphqlSystemRouterboard,
        ros::{DeviceDataCurrent, DeviceDataTarget},
    },
};
use async_graphql::{InputObject, Object, SimpleObject};
use mikrotik_model::{
    MikrotikDevice,
    hwconfig::DeviceType,
    model::{SystemIdentityCfg, SystemRouterboardState},
    resource::SingleResource,
};
use surge_ping::SurgeError;

pub struct GraphqlDeviceType(DeviceType);
impl From<DeviceType> for GraphqlDeviceType {
    fn from(value: DeviceType) -> Self {
        GraphqlDeviceType(value)
    }
}
#[Object]
impl GraphqlDeviceType {
    async fn name(&self) -> &str {
        self.0.device_type_name()
    }
}

struct GraphqlSystemIdentity<'a>(&'a SystemIdentityCfg);
#[Object]
impl GraphqlSystemIdentity<'_> {
    async fn name(&self) -> String {
        self.0.name.to_string()
    }
}
#[derive(Clone, Debug, SimpleObject)]
pub struct DeviceCfg {
    current: DeviceDataCurrent,
    target: DeviceDataTarget,
}

impl AccessibleDevice {
    pub async fn fetch_config(&self, client: &MikrotikDevice) -> Result<DeviceCfg, Error> {
        let current = DeviceDataCurrent::fetch(client).await?;
        let target = DeviceDataTarget::detect_device(client).await?;
        Ok(DeviceCfg { current, target })
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct DeviceStats {
    routerboard: GraphqlSystemRouterboard,
}
impl DeviceStats {
    pub async fn fetch(client: &MikrotikDevice) -> Result<DeviceStats, Error> {
        Ok(Self {
            routerboard: GraphqlSystemRouterboard(
                SystemRouterboardState::fetch(&client)
                    .await?
                    .expect("system/routerboard not found"),
            ),
        })
    }
}

#[Object]
impl DeviceDataCurrent {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}

#[Object]
impl DeviceDataTarget {
    async fn identity(&self) -> GraphqlSystemIdentity {
        GraphqlSystemIdentity(&self.identity)
    }
}
#[derive(InputObject)]
struct AdhocCredentials {
    username: Option<Box<str>>,
    #[graphql(secret)]
    password: Option<Box<str>>,
}
#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, SurgeError> {
        self.simple_ping(count.unwrap_or(1)).await
    }

    async fn device_stats(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<DeviceStats, Error> {
        let device = self
            .create_client(
                target.map(|v| str::parse(&v)).transpose()?,
                build_credential(credential_name, adhoc_credentials),
            )
            .await?;
        DeviceStats::fetch(&device).await
    }

    async fn config(
        &self,
        target: Option<String>,
        credential_name: Option<Box<str>>,
        adhoc_credentials: Option<AdhocCredentials>,
    ) -> Result<DeviceCfg, Error> {
        let client = self
            .create_client(
                target.map(|v| str::parse(&v)).transpose()?,
                build_credential(credential_name, adhoc_credentials),
            )
            .await?;
        self.fetch_config(&client).await
    }
}
fn build_credential(
    credential_name: Option<Box<str>>,
    adhoc_credentials: Option<AdhocCredentials>,
) -> Credentials {
    if let Some(name) = credential_name {
        Credentials::Named(name)
    } else if let Some(AdhocCredentials { username, password }) = adhoc_credentials {
        Credentials::Adhoc { username, password }
    } else {
        Credentials::Default
    }
}
