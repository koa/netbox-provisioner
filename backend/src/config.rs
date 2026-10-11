use config::{Config, ConfigError, Environment, File};
use lazy_static::lazy_static;
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
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
            .map(|u| Box::as_ref(u))
            .unwrap_or("admin")
    }
    pub fn password(&self) -> Option<&str> {
        self.password.as_ref().map(|p| Box::as_ref(p))
    }
}

impl Settings {
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
    let cfg = Config::builder()
        .add_source(File::with_name("config.yaml"))
        .add_source(Environment::with_prefix("app"))
        .build()?;
    let settings: Settings = cfg.get("settings")?;
    Ok(settings)
}

lazy_static! {
    pub static ref CONFIG: Settings = create_settings().expect("Cannot load config.yaml");
}
