use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
};

#[derive(Deserialize)]
pub struct Settings {
    auth_client_id: String,
    auth_issuer: String,
    /// OIDC scopes the frontend requests, space separated
    auth_scopes: Option<String>,
    pub auth_users: Box<[Box<str>]>,

    server_port: Option<u16>,
    server_mgmt_port: Option<u16>,
    server_bind_address: Option<IpAddr>,

    pub netbox_url: String,
    pub netbox_token: String,
    /// Writing to NetBox (the helpers): off unless set, and only with `netbox_write_token`.
    #[serde(default)]
    pub allow_netbox_write: bool,
    /// A NetBox token with write permission, used only by the helpers; `netbox_token` can
    /// stay read-only.
    pub netbox_write_token: Option<String>,

    pub mikrotik_credentials: HashMap<Box<str>, MikrotikCredentials>,

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
    pub api_networks: Option<Box<[ipnet::IpNet]>>,
    /// Where the jobs that go on after a restart are saved (`saved_job`), and the lock that
    /// keeps a second provisioner off them (default `state`, next to `config.yaml`).
    state_dir: Option<std::path::PathBuf>,
}

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

#[derive(Deserialize)]
pub struct MikrotikCredentials {
    user: Option<Box<str>>,
    password: Option<Box<str>>,
}

impl MikrotikCredentials {
    pub fn user(&self) -> &str {
        self.user
            .as_ref()
            .map(Box::as_ref)
            .unwrap_or("admin")
    }
    pub fn password(&self) -> Option<&str> {
        self.password.as_ref().map(Box::as_ref)
    }
}

impl Settings {
    pub fn apply_fallback(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.apply_fallback_seconds.unwrap_or(90))
    }
    pub fn update_fallback(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.update_fallback_seconds.unwrap_or(900))
    }
    pub fn state_dir(&self) -> &std::path::Path {
        self.state_dir.as_deref().unwrap_or(std::path::Path::new("state"))
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
mod test {
    use super::{environment, settings_from};
    use config::{File, FileFormat};

    const YAML: &str = "
settings:
  auth_client_id: client
  auth_issuer: https://id.example
  auth_users: [someone]
  netbox_url: https://netbox.example
  netbox_token: from-file
  mikrotik_credentials:
    lab:
      user: admin
";

    #[test]
    fn environment_overrides_single_values() {
        let env = [
            ("APP_SETTINGS__NETBOX_TOKEN", "from-env"),
            ("APP_SETTINGS__ALLOW_APPLY", "true"),
            ("APP_SETTINGS__SERVER_PORT", "9000"),
            ("APP_SETTINGS__MIKROTIK_CREDENTIALS__LAB__PASSWORD", "0123"),
            ("OTHER_SETTINGS__NETBOX_URL", "ignored"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let settings = settings_from(
            File::from_str(YAML, FileFormat::Yaml),
            environment().source(Some(env)),
        )
        .unwrap();
        assert_eq!(settings.netbox_token, "from-env");
        assert_eq!(settings.netbox_url, "https://netbox.example");
        assert!(settings.allow_apply);
        assert_eq!(settings.server_port(), 9000);
        let lab = &settings.mikrotik_credentials["lab"];
        assert_eq!((lab.user(), lab.password()), ("admin", Some("0123")));
    }

    #[test]
    fn api_networks_are_prefixes() {
        let yaml = format!("{YAML}  api_networks: [192.168.0.0/16, \"2a02:16a:fe00:8000::/50\"]\n");
        let settings = settings_from(File::from_str(&yaml, FileFormat::Yaml), environment()).unwrap();
        let networks = settings.api_networks.expect("api_networks");
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[1].to_string(), "2a02:16a:fe00:8000::/50");
    }

    /// Without `topics` the devices send what they sent before the provisioner.
    #[test]
    fn device_syslog_defaults_to_the_four_levels() {
        let yaml = format!("{YAML}  device_syslog:\n    remote: 2a02:16a:fe00:1a0::10:8\n");
        let settings = settings_from(File::from_str(&yaml, FileFormat::Yaml), environment()).unwrap();
        let syslog = settings.device_syslog.expect("device_syslog");
        assert_eq!(syslog.port, None);
        assert_eq!(syslog.topics().collect::<Vec<_>>(), ["info", "warning", "error", "critical"]);
    }
}
