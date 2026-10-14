extern crate core;

use crate::device::ros::SetupError;
use mikrotik_model::resource::{MissingDependenciesError, ResourceMutationError};
use thiserror::Error;

pub mod config;
pub mod context;
pub mod device;
pub mod error_info;
pub mod graphql;
pub mod job;
pub mod netbox;
pub mod topology;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error accessing device: {0}")]
    MikrotikApi(#[from] mikrotik_api::error::Error),
    #[error("Error on data model: {0}")]
    MikrotikModel(#[from] mikrotik_model::resource::Error),
    #[error("Error credentials")]
    MissingCredentials,
    #[error("Cannot parse ip address {0}")]
    AddressParse(#[from] std::net::AddrParseError),
    #[error("Cannot generate mutations: {0}")]
    ResourceMutation(#[from] ResourceMutationError),
    #[error("{0}")]
    MissingDependenciesError(Box<str>),
    #[error("Cannot write the script: {0}")]
    ErrorGeneratingString(#[from] std::fmt::Error),
    #[error("Cannot build setup {0}")]
    SetupError(#[from] SetupError),
    #[error("Cannot connect through the management proxy {0}: {1}")]
    Proxy(std::net::SocketAddr, tokio_socks::Error),
    #[error("Writing to devices is disabled (allow_apply)")]
    ApplyNotAllowed,
    #[error("No device {0}")]
    UnknownDevice(u32),
    #[error("No answer from {0}")]
    Timeout(std::net::IpAddr),
    #[error("{0} has no address in NetBox")]
    NoAddress(Box<str>),
    #[error("Console: {0}")]
    Terminal(#[from] device::terminal::TerminalError),
    #[error("No way known to set up {0}: {1}")]
    NoBootstrapPath(Box<str>, error_info::BootstrapObstacle),
    #[error("Setup of {0} failed: {1}")]
    Bootstrap(Box<str>, Box<str>),
    #[error("The device refused: {0}")]
    Refused(Box<str>),
    #[error("Update: {0}")]
    Update(Box<str>),
    #[error("{0} runs a job already")]
    Busy(Box<str>),
    #[error("No job {0}")]
    UnknownJob(u32),
    #[error("Cancelled")]
    Cancelled,
    #[error("Ping: {0}")]
    Ping(#[from] surge_ping::SurgeError),
}

impl From<MissingDependenciesError<'_, '_>> for Error {
    fn from(value: MissingDependenciesError) -> Self {
        Error::MissingDependenciesError(value.to_string().into_boxed_str())
    }
}
