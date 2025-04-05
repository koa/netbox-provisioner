use crate::{graphql::scalars::ScalarDuration, topology::access::DeviceAccess};
use async_graphql::{ComplexObject, Object, SimpleObject};
use lru::LruCache;
use mikrotik_model::{MikrotikDevice, model::SystemRouterboardState};
use std::{net::IpAddr, num::NonZeroUsize, sync::Arc, time::Duration};
use surge_ping::{IcmpPacket, SurgeError, ping};
use tokio::sync::Mutex;

pub mod ros;
#[derive(Debug, Default, Clone, PartialEq, Hash, Eq)]
pub enum Credentials {
    #[default]
    Default,
    Named(Box<str>),
    Adhoc {
        username: Option<Box<str>>,
        password: Option<Box<str>>,
    },
}
pub struct AccessibleDevice {
    address: IpAddr,
    credentials: Box<str>,
    clients: Arc<Mutex<LruCache<(IpAddr, Credentials), MikrotikDevice>>>,
    //     clients: Arc<Mutex<LruCache<(IpAddr, Credentials), MikrotikDevice>>>,
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
                clients: Arc::new(Mutex::new(LruCache::new(
                    NonZeroUsize::new(5).expect("5 should be not 0"),
                ))),
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
#[derive(Clone, Debug)]
struct GraphqlSystemRouterboard(SystemRouterboardState);

#[Object]
impl GraphqlSystemRouterboard {
    async fn device_type(&self) -> String {
        self.0.model.to_string()
    }
    async fn serial_number(&self) -> String {
        self.0.serial_number.to_string()
    }
    async fn firmware_type(&self) -> String {
        self.0.firmware_type.to_string()
    }
}
