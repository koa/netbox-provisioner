use crate::{
    Error, config::CONFIG, graphql::scalars::ScalarDuration, topology::access::device::DeviceAccess,
};
use async_graphql::{ComplexObject, Object, SimpleObject};
use mikrotik_api::{
    prelude::{MikrotikDevice as RawDevice, ParsedMessage},
    simple::SimpleResult,
};
use mikrotik_model::{MikrotikDevice, model::SystemRouterboardState};
use std::{net::IpAddr, sync::Arc, time::Duration};
use surge_ping::{IcmpPacket, SurgeError, ping};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_socks::tcp::Socks5Stream;

pub mod bootstrap;
pub mod power;
pub(crate) mod raw;
pub mod ros;
pub mod terminal;
pub mod update;
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum Credentials {
    Named(Box<str>),
    Adhoc {
        username: Option<Box<str>>,
        password: Option<Box<str>>,
    },
}
/// How a device is reached.
#[derive(Clone)]
pub enum Path {
    /// over IP: directly, or through the management proxy
    Routed,
    /// through an SSH tunnel to a neighbor on the same cable, which needs no route back on the
    /// device: how it is reached after its initial setup, until its first apply brings OSPF
    Neighbor(Arc<terminal::Tunnel>),
}

/// A connection to a device, whatever [`Path`] it takes.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Io for T {}

pub struct AccessibleDevice {
    address: IpAddr,
    path: Path,
    credentials: Credentials,
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
        path: Path,
        credentials: Credentials,
    ) -> Result<AccessibleDevice, Error> {
        let client = connect(address, &path, &credentials).await?;
        Ok(AccessibleDevice {
            address,
            path,
            credentials,
            client,
            device_config,
        })
    }
    /// A further connection to the same device, with another result type.
    pub(crate) async fn connect_again<D: ParsedMessage>(&self) -> Result<RawDevice<D>, Error> {
        connect(self.address, &self.path, &self.credentials).await
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
impl update::DeviceLink for AccessibleDevice {
    async fn api(&self) -> Result<RawDevice<SimpleResult>, Error> {
        self.connect_again().await
    }
    async fn upload(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        let (user, password) = resolve(&self.credentials)?;
        let stream = open_stream(self.address, &self.path, 22).await?;
        terminal::upload(stream, user, password, name, data).await?;
        Ok(())
    }
}

/// User and password of `credentials`.
fn resolve(credentials: &Credentials) -> Result<(&str, Option<&str>), Error> {
    Ok(match credentials {
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
    })
}

/// Logs in to the API of the device at `address` along `path`.
async fn connect<D: ParsedMessage>(
    address: IpAddr,
    path: &Path,
    credentials: &Credentials,
) -> Result<RawDevice<D>, Error> {
    let (username, password) = resolve(credentials)?;
    let stream = open_stream(address, path, 8728).await?;
    Ok(RawDevice::connect_with_stream(stream, username.as_bytes(), password.map(str::as_bytes)).await?)
}

/// A connection to `port` at `address` along `path`: routed through the management proxy
/// unless it is the proxy, or made by the neighbor.
async fn open_stream(address: IpAddr, path: &Path, port: u16) -> Result<Box<dyn Io>, Error> {
    Ok(match path {
        Path::Neighbor(tunnel) => Box::new(tunnel.open(address, port).await?),
        Path::Routed => match proxied_stream(address, port).await? {
            Some(stream) => Box::new(stream),
            None => {
                let stream = TcpStream::connect((address, port))
                    .await
                    .map_err(|e| terminal::TerminalError::Ssh(e.into()))?;
                let _ = stream.set_nodelay(true);
                Box::new(stream)
            }
        },
    })
}
/// A connection to `port` at `address` through the management proxy; `None` if it is to be
/// reached directly (no proxy configured, or it is the proxy itself).
///
/// RouterOS answers "general failure" for an unreachable target, but now and then also for a
/// reachable one (seen right after another connection to it), so that is tried twice more.
pub async fn proxied_stream(
    address: IpAddr,
    port: u16,
) -> Result<Option<Socks5Stream<TcpStream>>, Error> {
    let Some(proxy) = CONFIG
        .management_proxy
        .as_ref()
        .filter(|proxy| proxy.address.ip() != address)
    else {
        return Ok(None);
    };
    let mut attempt = 0;
    loop {
        match Socks5Stream::connect_with_password(
            proxy.address,
            (address, port),
            &proxy.user,
            &proxy.password,
        )
        .await
        {
            Ok(stream) => return Ok(Some(stream)),
            Err(tokio_socks::Error::GeneralSocksServerFailure) if attempt < 2 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => return Err(Error::Proxy(proxy.address, e)),
        }
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
