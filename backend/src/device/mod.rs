use crate::{
    Error, config::CONFIG, graphql::scalars::ScalarDuration, topology::access::device::DeviceAccess,
};
use async_graphql::{ComplexObject, Object, SimpleObject};
use mikrotik_model::{MikrotikDevice, model::SystemRouterboardState};
use std::{net::IpAddr, time::Duration};
use surge_ping::{IcmpPacket, SurgeError, ping};

pub mod ros;
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum Credentials {
    Named(Box<str>),
    Adhoc {
        username: Option<Box<str>>,
        password: Option<Box<str>>,
    },
}
pub struct AccessibleDevice {
    address: IpAddr,
    client: MikrotikDevice,
    device_config: DeviceAccess,
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

impl AccessibleDevice {
    pub async fn create_client(
        device_config: DeviceAccess,
        address: IpAddr,
        credentials: Credentials,
    ) -> Result<AccessibleDevice, Error> {
        Ok({
            let (username, password) = match &credentials {
                Credentials::Named(name) => {
                    let c = CONFIG
                        .mikrotik_credentials
                        .get(name.as_ref())
                        .ok_or(Error::MissingCredentials)?;
                    (c.user(), c.password())
                }
                Credentials::Adhoc { username, password } => (
                    username.as_ref().map(Box::as_ref).unwrap_or("admin"),
                    password.as_ref().map(Box::as_ref),
                ),
            };
            let mikrotik_device = MikrotikDevice::connect(
                (address, 8728),
                username.as_bytes(),
                password.map(|p| p.as_bytes()),
            )
            .await?;
            AccessibleDevice {
                address,
                client: mikrotik_device,
                device_config,
            }
        })
    }

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
