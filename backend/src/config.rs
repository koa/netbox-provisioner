//! Scope: The settings of `config.yaml` and the environment, read once; not what is derived from
//! them per device (`device`, `topology`).
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
};

#[derive(Deserialize)]
pub struct Settings {
    auth_client_id: Box<str>,
    auth_issuer: Box<str>,
    /// OIDC scopes the frontend requests, space separated
    auth_scopes: Option<String>,
    pub auth_users: Box<[Box<str>]>,

    server_port: Option<u16>,
    server_mgmt_port: Option<u16>,
    server_bind_address: Option<IpAddr>,

    pub netbox_url: Box<str>,
    pub netbox_token: Box<str>,
    /// Writing to NetBox (the helpers): off unless set, and only with `netbox_write_token`.
    #[serde(default)]
    pub allow_netbox_write: bool,
    /// A NetBox token with write permission, used only by the helpers; `netbox_token` can
    /// stay read-only.
    pub netbox_write_token: Option<String>,
    /// The secret NetBox signs its webhook's calls with (`/netbox-webhook`, the automatic
    /// rollout, `X-Hook-Signature`); without, no call is taken.
    pub netbox_webhook_secret: Option<Box<str>>,

    pub mikrotik_credentials: HashMap<Box<str>, MikrotikCredentials>,
    /// with it, WebFig goes over HTTPS with a certificate of this CA on every device, and plain
    /// HTTP (`www`) off (A7)
    pub www_ca: Option<WwwCa>,
    /// OSPF authentication of the segments (A10, `docs/ospf-auth.md`); without, no
    /// credentials on a device are touched.
    pub ospf_auth: Option<OspfAuth>,

    /// The device every other device is reached through, e.g. the entry router of a network
    /// not routed to the provisioner.
    pub management_proxy: Option<ManagementProxy>,
    /// Writing to devices: off unless set, since the diff reconciles the whole device and
    /// would remove everything NetBox doesn't know of (no managed-marker scheme yet).
    #[serde(default)]
    pub allow_apply: bool,
    /// The RouterOS channel preselected for updates (`long-term` (default), `stable`,
    /// `testing`, `development`).
    pub routeros_channel: Option<crate::device::update::Channel>,
    /// Seconds after the end of the apply script within which the provisioner has to reach
    /// the device again, or it loads the backup from before (default 90). The script brings
    /// the fallback forward to this at its end; one that never ends has `SCRIPT_LIMIT` (120 s)
    /// more.
    apply_fallback_seconds: Option<u64>,
    /// Seconds after a boot after which a device updated with two partitions switches back to
    /// the RouterOS from before, unless the provisioner reached it (default 900). Longer than
    /// the provisioner waits for a reboot (10 min), so a slow update isn't taken back.
    update_fallback_seconds: Option<u64>,
    /// The syslog server every device logs to, in CEF; without it the devices log only
    /// locally (RouterOS's own rules).
    pub device_syslog: Option<DeviceSyslog>,
    /// The networks the plain API (`/ip/service api`, port 8728) and WebFig (`www`) of every
    /// device answer;
    /// unset or empty: the device's own setting stays (RouterOS's: all). It must hold the
    /// address the provisioner comes from on each device (its own, or the management proxy's
    /// towards the device), or the apply locks it out and the fallback takes the apply back.
    /// The management proxy's SOCKS takes only these sources too.
    pub api_networks: Option<Box<[ipnet::IpNet]>>,
    /// Whether a scrape of `/metrics` reads every device (`crate::metrics`, default `true`);
    /// `false` serves the provisioner's own metrics only.
    device_metrics: Option<bool>,
    /// Where the jobs that go on after a restart are saved (`saved_job`), and the lock that
    /// keeps a second provisioner off them (default `state`, next to `config.yaml`).
    state_dir: Option<std::path::PathBuf>,
    /// What a device pings to see whether it reaches the internet, IPv4 and IPv6 (the
    /// automatic rollout, `docs/auto-rollout.md`): a family is reached when one of
    /// its targets answers. Default: the anycast DNS resolvers of Cloudflare, Quad9 and Google.
    internet_targets: Option<Box<[IpAddr]>>,
    /// Below which signal a WLAN client is a finding on the page „Zustand“ (dBm, as the AP
    /// hears it): `weak` (default -75) a warning, `bad` (default -80) an error.
    #[serde(default)]
    wlan_signal: WlanSignalSettings,
}

#[derive(Deserialize, Default)]
struct WlanSignalSettings {
    weak: Option<i16>,
    bad: Option<i16>,
}

/// Below which signal a WLAN client is weak or bad, dBm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalThresholds {
    pub weak: i16,
    pub bad: i16,
}

/// [`Settings::internet_targets`] without the setting.
const INTERNET_TARGETS: [IpAddr; 6] = [
    IpAddr::V4(std::net::Ipv4Addr::new(1, 1, 1, 1)),
    IpAddr::V4(std::net::Ipv4Addr::new(9, 9, 9, 9)),
    IpAddr::V4(std::net::Ipv4Addr::new(8, 8, 8, 8)),
    IpAddr::V6(std::net::Ipv6Addr::new(
        0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111,
    )),
    IpAddr::V6(std::net::Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0xfe)),
    IpAddr::V6(std::net::Ipv6Addr::new(
        0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888,
    )),
];

#[derive(Deserialize)]
pub struct DeviceSyslog {
    pub remote: IpAddr,
    /// UDP port, default 514
    pub port: Option<u16>,
    /// the topics sent, one rule each; default `info`, `warning`, `error`, `critical`
    pub topics: Option<Box<[Box<str>]>>,
}

impl DeviceSyslog {
    pub fn topics(&self) -> impl Iterator<Item = &str> {
        self.topics
            .as_deref()
            .map(|topics| topics.iter().map(Box::as_ref).collect::<Vec<_>>())
            .unwrap_or_else(|| crate::device::ros::syslog::DEFAULT_TOPICS.to_vec())
            .into_iter()
    }
}

/// A RouterOS device acting as SOCKS5 proxy (`/ip socks`) for the provisioner. Its own
/// configuration enables the proxy for exactly this user; the device itself is connected to
/// directly at `address`.
#[derive(Deserialize)]
pub struct ManagementProxy {
    /// name of the device in NetBox
    pub device: Box<str>,
    pub address: SocketAddr,
    pub user: Box<str>,
    pub password: Box<str>,
}

/// The CA issuing the devices' certificates for WebFig over HTTPS (A7), all PEM: `certificate`
/// the issuing CA's, then the ones above it (a device sends them along with its own, but the
/// root), `key` its key, `root` the root CA's (without: the self-signed one of `certificate`),
/// which the admins download from the provisioner (`/www-ca.crt`) and import into their
/// browsers once.
#[derive(Deserialize)]
pub struct WwwCa {
    pub certificate: Box<str>,
    pub key: Box<str>,
    pub root: Option<Box<str>>,
}

/// The secrets the segments' OSPF keys come from, by version, and the version they go to
/// (`device::ospf_auth`). The version is `auth-id` on the devices, so a segment shows which it
/// has; a secret may go once no segment has its version.
#[derive(Deserialize)]
#[serde(try_from = "OspfAuthSettings")]
pub struct OspfAuth {
    current: u8,
    secrets: HashMap<u8, Box<str>>,
}

/// [`OspfAuth`] as written: the versions as keys of a map, which come as text from the
/// environment (`APP_SETTINGS__OSPF_AUTH__SECRETS__2`).
#[derive(Deserialize)]
struct OspfAuthSettings {
    current: u8,
    secrets: HashMap<Box<str>, Box<str>>,
}

impl TryFrom<OspfAuthSettings> for OspfAuth {
    type Error = String;
    fn try_from(settings: OspfAuthSettings) -> Result<Self, String> {
        let secrets = settings
            .secrets
            .into_iter()
            .map(|(version, secret)| match version.parse::<u8>() {
                Ok(number) if number > 0 && !secret.is_empty() => Ok((number, secret)),
                Ok(number) if number > 0 => Err(format!("ospf_auth.secrets.{version}: empty")),
                _ => Err(format!(
                    "ospf_auth.secrets.{version}: a version is 1 to 255"
                )),
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        if !secrets.contains_key(&settings.current) {
            return Err(format!(
                "ospf_auth.current: no secret of version {}",
                settings.current
            ));
        }
        Ok(Self {
            current: settings.current,
            secrets,
        })
    }
}

impl OspfAuth {
    /// One version with its secret, the current one (tests, the CHR harness).
    pub fn new(current: u8, secret: &str) -> Self {
        Self {
            current,
            secrets: [(current, secret.into())].into(),
        }
    }
    /// The version the segments go to.
    pub fn current(&self) -> u8 {
        self.current
    }
    /// The secret of the version the segments go to.
    pub fn current_secret(&self) -> &str {
        &self.secrets[&self.current]
    }
}

#[derive(Deserialize)]
pub struct MikrotikCredentials {
    user: Option<Box<str>>,
    password: Option<Box<str>>,
    /// the password before a change, while devices may still have it: a login with it goes,
    /// and the device's apply sets `password` (A9)
    previous_password: Option<Box<str>>,
}

impl MikrotikCredentials {
    pub fn user(&self) -> &str {
        self.user.as_ref().map(Box::as_ref).unwrap_or("admin")
    }
    pub fn password(&self) -> Option<&str> {
        self.password.as_ref().map(Box::as_ref)
    }
    pub fn previous_password(&self) -> Option<&str> {
        self.previous_password.as_ref().map(Box::as_ref)
    }
}

impl Settings {
    pub fn apply_fallback(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.apply_fallback_seconds.unwrap_or(90))
    }
    /// Whether `/metrics` reads the devices.
    pub fn device_metrics(&self) -> bool {
        self.device_metrics.unwrap_or(true)
    }
    pub fn update_fallback(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.update_fallback_seconds.unwrap_or(900))
    }
    pub fn wlan_signal(&self) -> SignalThresholds {
        SignalThresholds {
            weak: self.wlan_signal.weak.unwrap_or(-75),
            bad: self.wlan_signal.bad.unwrap_or(-80),
        }
    }
    pub fn internet_targets(&self) -> &[IpAddr] {
        self.internet_targets
            .as_deref()
            .unwrap_or(&INTERNET_TARGETS)
    }
    pub fn state_dir(&self) -> &std::path::Path {
        self.state_dir
            .as_deref()
            .unwrap_or(std::path::Path::new("state"))
    }
    pub fn auth_client_id(&self) -> &str {
        &self.auth_client_id
    }
    pub fn auth_issuer(&self) -> &str {
        &self.auth_issuer
    }
    pub fn auth_scopes(&self) -> Box<[Box<str>]> {
        self.auth_scopes
            .as_deref()
            .unwrap_or("openid profile email offline_access")
            .split_whitespace()
            .map(Box::from)
            .collect()
    }
    pub fn server_port(&self) -> u16 {
        self.server_port.unwrap_or(8080)
    }
    pub fn server_mgmt_port(&self) -> u16 {
        self.server_mgmt_port
            .unwrap_or_else(|| self.server_port() + 1000)
    }
    pub fn server_bind_address(&self) -> IpAddr {
        self.server_bind_address
            .unwrap_or_else(|| IpAddr::from([0u8; 16]))
    }
}

fn create_settings() -> Result<Settings, ConfigError> {
    settings_from(File::with_name("config.yaml"), environment())
}

/// `APP_SETTINGS__NETBOX_TOKEN` overrides `settings.netbox_token` of `config.yaml`, e.g. from a
/// Kubernetes secret; `__` separates the levels, since the keys contain `_` themselves. Keys
/// come out lowercase, so a `mikrotik_credentials` entry set this way needs a lowercase name.
fn environment() -> Environment {
    Environment::with_prefix("app")
        .prefix_separator("_")
        .separator("__")
}

fn settings_from(
    file: impl config::Source + Send + Sync + 'static,
    environment: Environment,
) -> Result<Settings, ConfigError> {
    Config::builder()
        .add_source(file)
        .add_source(environment)
        .build()?
        .get("settings")
}

pub static CONFIG: LazyLock<Settings> =
    LazyLock::new(|| create_settings().expect("Cannot load config.yaml"));

#[cfg(test)]
mod test;
