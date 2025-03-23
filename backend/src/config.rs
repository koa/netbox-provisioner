use std::net::IpAddr;

use config::{Config, ConfigError, Environment, File};
use lazy_static::lazy_static;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Settings {
    auth_client_id: String,
    auth_issuer: String,
    auth_token_url: Option<String>,
    auth_url: Option<String>,
    pub auth_users: Box<[Box<str>]>,

    server_port: Option<u16>,
    server_mgmt_port: Option<u16>,
    server_bind_address: Option<IpAddr>,

    pub netbox_url: String,
    pub netbox_token: String,
}

impl Settings {
    pub fn auth_client_id(&self) -> &str {
        &self.auth_client_id
    }
    pub fn auth_issuer(&self) -> &str {
        &self.auth_issuer
    }
    pub fn auth_token_url(&self) -> String {
        self.auth_token_url
            .clone()
            .unwrap_or_else(|| format!("{}/protocol/openid-connect/token", self.auth_issuer))
    }
    pub fn auth_url(&self) -> String {
        self.auth_url
            .clone()
            .unwrap_or_else(|| format!("{}/protocol/openid-connect/auth", self.auth_issuer))
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
