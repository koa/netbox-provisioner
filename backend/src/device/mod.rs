use crate::{
    Error,
    config::CONFIG,
    facts::{self, Came, Fact, FactKey, FactKind, Freshness},
    graphql::scalars::ScalarDuration,
    topology::access::device::DeviceAccess,
};
use async_graphql::{ComplexObject, Object, SimpleObject};
use mikrotik_api::{
    prelude::{MikrotikDevice as RawDevice, ParsedMessage},
    simple::SimpleResult,
};
use mikrotik_model::{
    MikrotikDevice,
    model::{SystemIdentityCfg, SystemRouterboardState},
    resource::SingleResource,
};
use std::{net::IpAddr, sync::Arc, time::Duration};
use surge_ping::{IcmpPacket, SurgeError, ping};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_socks::tcp::Socks5Stream;

pub mod batch;
pub mod bootstrap;
pub mod dependency;
pub mod device_mode;
pub mod interface_live;
pub mod leftovers;
pub mod neighbors;
pub mod partitions;
pub mod power_cycle;
pub(crate) mod radio_hold;
pub(crate) mod raw;
pub mod rollout;
pub mod ros;
pub mod site_diff;
pub mod terminal;
pub mod update;
pub mod update_fallback;
pub mod watchdog;
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum Credentials {
    Named(Box<str>),
    Adhoc {
        username: Option<Box<str>>,
        password: Option<Box<str>>,
    },
}
impl Credentials {
    /// the name of credentials by name (`mikrotik_credentials`)
    pub fn name(&self) -> Option<&str> {
        match self {
            Credentials::Named(name) => Some(name),
            Credentials::Adhoc { .. } => None,
        }
    }
    /// What tells them apart in a fact's key: the name, or a hash of what was typed in (never
    /// the password itself).
    pub(crate) fn fact_key(&self) -> String {
        use std::hash::{DefaultHasher, Hash, Hasher};
        match self {
            Credentials::Named(name) => format!("credentials {name}"),
            Credentials::Adhoc { .. } => {
                let mut hasher = DefaultHasher::new();
                self.hash(&mut hasher);
                format!("typed in {:016x}", hasher.finish())
            }
        }
    }
}

/// How a device is reached.
#[derive(Clone)]
pub enum Path {
    /// over IP: directly, or through the management proxy
    Routed,
    /// through an SSH tunnel to a neighbor on the same cable, which needs no route back on the
    /// device: how it is reached after its initial setup, until its first apply brings OSPF.
    /// Each connection gets its own SSH session: of two API logins at once over one session
    /// through RouterOS (lab-np2 to lab-sxt-a, 7.23.8) the second got no answer 4 times in 5,
    /// though the device logged it in (link-5-station's log, over router-stall-burri); with
    /// a session each, 15 of 15 went through.
    Neighbor(Arc<bootstrap::BootstrapPlan>),
}

impl Path {
    /// the label of the metrics
    fn label(&self) -> &'static str {
        match self {
            Path::Routed => "routed",
            Path::Neighbor(_) => "neighbor",
        }
    }
    /// what tells it apart in a fact's key
    fn fact_key(&self) -> String {
        match self {
            Path::Routed => "routed".into(),
            Path::Neighbor(plan) => format!("over {} at {}", plan.neighbor, plan.neighbor_address),
        }
    }
}

/// A login to a device's API: the connection, and the identity of the device answering.
#[derive(Clone)]
pub struct Login {
    client: MikrotikDevice,
    identity: Box<str>,
}

/// How long a login that went is taken as the way to the device (a page loaded again, a
/// neighbor checked for a setup): the connection is checked before it is used again.
const LOGIN_WENT: Duration = Duration::from_secs(60);
/// How long a refused login is taken as refused: it is until someone changes the credentials,
/// and every try counts against RouterOS' SSH blacklist.
const LOGIN_REFUSED: Duration = Duration::from_secs(10 * 60);
/// How long another device answering at the address is taken as answering there.
const LOGIN_WRONG_DEVICE: Duration = Duration::from_secs(60);
/// How long a login without answer is taken as without: a cable may just have been plugged in.
const LOGIN_NO_ANSWER: Duration = Duration::from_secs(20);
/// How long a login may take, connection included; through the management proxy a missing
/// route fails at once, a host that is down only after the TCP timeout.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The login to `device` at `address` along `path` with `credentials`, as a fact
/// (`crate::facts`): a login that went a moment ago gives its connection again (checked
/// first), one refused or unanswered isn't tried again for a while ([`LOGIN_REFUSED`],
/// [`LOGIN_NO_ANSWER`]), and two at once are one.
pub(crate) async fn login(
    device: &DeviceAccess,
    address: IpAddr,
    path: &Path,
    credentials: &Credentials,
) -> Fact<Login> {
    let key = FactKey::new(
        FactKind::Login,
        Some(device.id()),
        format!("{address} {} {}", path.fact_key(), credentials.fact_key()),
    )
    .via(path.label());
    let ask = || {
        let (device, path, credentials) = (device.clone(), path.clone(), credentials.clone());
        async move {
            tokio::time::timeout(CONNECT_TIMEOUT, async {
                let client = connect(address, &path, &credentials).await?;
                let identity: Box<str> = SystemIdentityCfg::fetch(&client)
                    .await?
                    .map(|identity| identity.name.to_string().into())
                    .unwrap_or_default();
                if let Some(other) = other_device_named(&device, &identity) {
                    return Err(Error::WrongDevice {
                        device: device.name().into(),
                        address: address.to_string().into(),
                        other,
                    });
                }
                Ok(Login { client, identity })
            })
            .await
            .unwrap_or(Err(Error::Timeout(address)))
        }
    };
    let fact = facts::get_judged(
        key.clone(),
        Freshness::MaxAge(Duration::ZERO),
        judge_login,
        ask(),
    )
    .await;
    if let (Came::Hit, Ok(login)) = (fact.came, &fact.value) {
        // the connection may have gone since (a reboot, a tunnel closed)
        let alive = tokio::time::timeout(
            Duration::from_secs(5),
            SystemIdentityCfg::fetch(&login.client),
        )
        .await;
        if !matches!(alive, Ok(Ok(_))) {
            facts::forget(&key);
            return facts::get_judged(key, Freshness::MaxAge(Duration::ZERO), judge_login, ask())
                .await;
        }
    }
    fact
}

/// How old a login may be, by what it came to.
fn judge_login(login: &Result<Arc<Login>, Arc<Error>>) -> Option<Duration> {
    use mikrotik_api::error::Error as ApiError;
    Some(match login {
        Ok(_) => LOGIN_WENT,
        Err(error) => match error.root() {
            Error::MikrotikApi(ApiError::LoginFailed) | Error::MissingCredentials => LOGIN_REFUSED,
            Error::WrongDevice { .. } => LOGIN_WRONG_DEVICE,
            _ => LOGIN_NO_ANSWER,
        },
    })
}

/// Whether a login to `device` at `address` along `path` went within [`LOGIN_WENT`]: such a
/// way is tried first.
pub(crate) fn went_lately(
    device: &DeviceAccess,
    address: IpAddr,
    path: &Path,
    credentials: &Credentials,
) -> bool {
    let key = FactKey::new(
        FactKind::Login,
        Some(device.id()),
        format!("{address} {} {}", path.fact_key(), credentials.fact_key()),
    );
    facts::peek::<Login>(&key).is_some_and(|fact| fact.value.is_ok() && fact.age() <= LOGIN_WENT)
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
    /// `/system/identity` of the device answering at `address`
    identity: Box<str>,
    /// an apply changing a radio suspended its watch address (`radio_hold`): the checks after
    /// it keep what the device has there
    watchdog_held: std::sync::atomic::AtomicBool,
    /// the youngest age the request takes facts at (`access(maxAge:)`), below the resolvers'
    /// own
    max_age: Option<Duration>,
    /// generating for a helper's plan (`generating_for`): its facts are its own
    preview: bool,
}

/// The device a connection reached. Only the identity tells it apart from another one at the
/// same address; [`AccessibleDevice::create_client`] refuses one named like another NetBox
/// device, a different identity (a factory `MikroTik`, a name NetBox doesn't know) only shows.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Connected {
    /// the device in NetBox it was looked for as
    pub device: Box<str>,
    pub address: Box<str>,
    /// the neighbor whose SSH tunnel the connection goes through, `None` when routed
    pub neighbor: Option<Box<str>>,
    pub identity: Box<str>,
    /// the identity is the device's name in NetBox
    pub same_name: bool,
}

/// A job connects to `device`: the one it works on, or one powering it.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct Connecting {
    pub device: Box<str>,
}

/// The device other than `device` NetBox has by the name `identity`.
pub(crate) fn other_device_named(device: &DeviceAccess, identity: &str) -> Option<Box<str>> {
    device
        .topology()
        .list_devices()
        .find(|other| other.id() != device.id() && other.name() == identity)
        .map(|other| other.name().into())
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
    /// The same connection, generating for `device` instead: the device as NetBox has it
    /// after a helper's plan (`NetboxPlan.previewAccess`).
    pub(crate) fn generating_for(mut self, device: DeviceAccess) -> Self {
        self.device_config = device;
        // what it works out from the planned NetBox isn't the device's fact
        self.preview = true;
        self.max_age = Some(Duration::ZERO);
        self
    }
    /// Logs in to `device_config` at `address` along `path` ([`login`]: a login that went a
    /// moment ago is taken again, one refused or unanswered fails at once for a while).
    pub async fn create_client(
        device_config: DeviceAccess,
        address: IpAddr,
        path: Path,
        credentials: Credentials,
    ) -> Result<AccessibleDevice, Error> {
        let login = login(&device_config, address, &path, &credentials)
            .await
            .result()?;
        Ok(AccessibleDevice {
            address,
            path,
            credentials,
            client: login.client.clone(),
            device_config,
            identity: login.identity.clone(),
            watchdog_held: Default::default(),
            max_age: None,
            preview: false,
        })
    }
    /// The same, taking facts at most `max_age` old (`access(maxAge:)`).
    pub(crate) fn with_max_age(mut self, max_age: Option<Duration>) -> Self {
        self.max_age = max_age;
        self
    }
    /// Another handle on the same connection, for work that outlives the request (a fact
    /// asked on its own, `facts::get`).
    pub(crate) fn duplicate(&self) -> AccessibleDevice {
        use std::sync::atomic::{AtomicBool, Ordering};
        AccessibleDevice {
            address: self.address,
            path: self.path.clone(),
            credentials: self.credentials.clone(),
            client: self.client.clone(),
            device_config: self.device_config.clone(),
            identity: self.identity.clone(),
            watchdog_held: AtomicBool::new(self.watchdog_held.load(Ordering::SeqCst)),
            max_age: self.max_age,
            preview: self.preview,
        }
    }
    /// The device actually answering: where, how and with which identity.
    pub fn connected_device(&self) -> Connected {
        Connected {
            device: self.device_config.name().into(),
            address: self.address.to_string().into(),
            neighbor: match &self.path {
                Path::Routed => None,
                Path::Neighbor(plan) => Some(plan.neighbor.as_str().into()),
            },
            identity: self.identity.clone(),
            same_name: *self.identity == *self.device_config.name(),
        }
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
    async fn upload(
        &self,
        name: &str,
        data: &[u8],
        progress: &(dyn Fn(usize) + Sync),
    ) -> Result<(), Error> {
        let (user, password) = resolve(&self.credentials)?;
        let stream = open_stream(self.address, &self.path, 22).await?;
        terminal::upload(stream, user, password, name, data, progress).await?;
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
pub(crate) async fn connect<D: ParsedMessage>(
    address: IpAddr,
    path: &Path,
    credentials: &Credentials,
) -> Result<RawDevice<D>, Error> {
    let (username, password) = resolve(credentials)?;
    let Path::Neighbor(plan) = path else {
        let stream = open_stream(address, path, 8728).await?;
        return Ok(RawDevice::connect_with_stream(
            stream,
            username.as_bytes(),
            password.map(str::as_bytes),
        )
        .await?);
    };
    for attempt in 1..=NEIGHBOR_LOGIN_TRIES {
        let stream = open_stream(address, path, 8728).await?;
        let login = RawDevice::connect_with_stream(
            stream,
            username.as_bytes(),
            password.map(str::as_bytes),
        );
        match tokio::time::timeout(NEIGHBOR_LOGIN_TIMEOUT, login).await {
            Ok(connected) => return Ok(connected?),
            Err(_) => log::info!(
                "{address}: no answer to the login over {} ({attempt})",
                plan.neighbor
            ),
        }
    }
    Err(Error::NeighborNoAnswer(
        plan.neighbor.as_str().into(),
        address,
    ))
}

/// How long a login over a neighbor may take, each of [`NEIGHBOR_LOGIN_TRIES`]: across the
/// cable the device answers within a second. RouterOS takes a forward also when its connection
/// doesn't come about (CHR 7.24.5: a firewall dropping it, a route elsewhere), which leaves the
/// login without an answer; of five logins at once over berghof-burri, three never reached
/// berghof-hansueli (7.21.5, its log), the others went through.
const NEIGHBOR_LOGIN_TIMEOUT: Duration = Duration::from_secs(10);
const NEIGHBOR_LOGIN_TRIES: u32 = 3;

/// A connection to `port` at `address` along `path`: routed through the management proxy
/// unless it is the proxy, or made by the neighbor.
async fn open_stream(address: IpAddr, path: &Path, port: u16) -> Result<Box<dyn Io>, Error> {
    Ok(match path {
        // boxed: the tunnel reads the neighbor's `/ip ssh` over a routed connection first
        Path::Neighbor(plan) => Box::new(
            Box::pin(bootstrap::tunnel(plan))
                .await?
                .into_stream(address, port)
                .await?,
        ),
        Path::Routed => match proxied_stream(address, port).await? {
            Some(stream) => Box::new(stream),
            None => {
                let stream =
                    tokio::time::timeout(OPEN_TIMEOUT, TcpStream::connect((address, port)))
                        .await
                        .map_err(|_| Error::Timeout(address))?
                        .map_err(|e| terminal::TerminalError::Ssh(e.into()))?;
                let _ = stream.set_nodelay(true);
                Box::new(stream)
            }
        },
    })
}

/// How long opening a connection may take, directly or through the management proxy (each
/// try of it).
pub(crate) const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
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
        let connected = tokio::time::timeout(
            OPEN_TIMEOUT,
            Socks5Stream::connect_with_password(
                proxy.address,
                (address, port),
                &proxy.user,
                &proxy.password,
            ),
        )
        .await
        .map_err(|_| Error::Timeout(address))?;
        match connected {
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

#[cfg(test)]
mod tests {
    use super::other_device_named;
    use crate::topology::{Device, TopologyHolder, builder::TopologyBuilder};

    /// a factory identity or one NetBox doesn't know only shows; the name of another NetBox
    /// device is that device (berghof-haus-stall's address answered by another one)
    #[tokio::test]
    async fn an_identity_naming_another_device_is_that_device() {
        let mut builder = TopologyBuilder::default();
        let mut ids = Vec::new();
        for name in ["berghof-haus-stall", "link-5-ap"] {
            let id = builder.devices.next_id();
            builder.devices.insert(
                id,
                Device {
                    name: name.into(),
                    ..Default::default()
                },
            );
            ids.push(id);
        }
        let holder = Into::<TopologyHolder>::into(builder);
        let device = holder.devices_by_id(ids[0]).await.unwrap();
        assert_eq!(other_device_named(&device, "berghof-haus-stall"), None);
        assert_eq!(other_device_named(&device, "MikroTik"), None);
        assert_eq!(
            other_device_named(&device, "link-5-ap").as_deref(),
            Some("link-5-ap")
        );
    }
}
