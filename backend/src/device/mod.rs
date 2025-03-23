use crate::{
    Error, device::ros::GraphqlDeviceType, graphql::scalars::ScalarDuration, topology::DeviceAccess,
};
use async_graphql::{ComplexObject, Object, SimpleObject};
use mikrotik_model::{MikrotikDevice, hwconfig::DeviceType, model::SystemRouterboardState};
use std::{net::IpAddr, sync::Arc, time::Duration};
use surge_ping::{IcmpPacket, SurgeError, ping};
use tokio::sync::Mutex;

pub mod ros;

pub struct AccessibleDevice {
    address: IpAddr,
    credentials: Box<str>,
    default_client: Arc<Mutex<Option<MikrotikDevice>>>,
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct PingResult {
    ttl: Option<u8>,
    #[graphql(skip)]
    duration: Duration,
}
#[ComplexObject]
impl PingResult {
    async fn duration(&self) -> ScalarDuration {
        self.duration.into()
    }
}

impl From<DeviceAccess> for Option<AccessibleDevice> {
    fn from(value: DeviceAccess) -> Option<AccessibleDevice> {
        Option::zip(value.primary_ip(), value.credentials()).map(|(address, credentials)| {
            AccessibleDevice {
                address,
                credentials: Box::from(credentials),
                default_client: Default::default(),
            }
        })
    }
}

impl AccessibleDevice {
    pub async fn simple_ping(&self, count: u8) -> Result<Box<[PingResult]>, SurgeError> {
        let mut result = Vec::new();
        for _i in 0..count {
            let (packet, duration) = ping(self.address, &[]).await?;
            let ttl = match packet {
                IcmpPacket::V4(v4) => v4.get_ttl(),
                IcmpPacket::V6(v6) => Some(v6.get_max_hop_limit()),
            };
            result.push(PingResult { ttl, duration })
        }
        Ok(result.into_boxed_slice())
    }
}

#[Object]
impl AccessibleDevice {
    async fn ping(&self, count: Option<u8>) -> Result<Box<[PingResult]>, SurgeError> {
        self.simple_ping(count.unwrap_or(1)).await
    }
    async fn detect_device(&self) -> Result<GraphqlDeviceType, Error> {
        let device = self.get_default_client().await?;
        let routerboard =
            <SystemRouterboardState as mikrotik_model::resource::SingleResource>::fetch(&device)
                .await?
                .expect("system/routerboard not found");
        DeviceType::type_by_name(&routerboard.model.0)
            .map(|d| d.into())
            .ok_or(Error::MikrotikModel(
                mikrotik_model::resource::Error::UnknownType(routerboard.model),
            ))
    }
}
