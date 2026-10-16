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
    /// boxed: it is large (over 2 KB), and every `Result<_, Error>` would be as large
    ResourceMutation(Box<ResourceMutationError>),
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
    /// over mac-telnet to these MACs, with every credentials tried
    #[error("Login over mac-telnet to {0} refused")]
    ConsoleLoginRefused(Box<str>),
    /// over mac-telnet to these MACs, within the login timeout
    #[error("No answer to the login over mac-telnet to {0}")]
    ConsoleNoAnswer(Box<str>),
    /// a setup command (passwords replaced by `***`) and the error the console printed
    #[error("Setup command failed: {0}: {1}")]
    SetupCommand(Box<str>, Box<str>),
    #[error("The mac-telnet session ended before {0}")]
    ConsoleSessionEnded(Box<str>),
    #[error("The device refused: {0}")]
    Refused(Box<str>),
    /// the partitions the device has (one is needed, with the device mode allowing more)
    #[error("Cannot split the flash: {0} partitions, or the device mode doesn't allow partitions")]
    CannotRepartition(u32),
    #[error("The device-mode change was not confirmed at the device in time")]
    DeviceModeNotConfirmed,
    /// the features still off
    #[error("The device runs without {0} in its device mode")]
    DeviceModeUnchanged(Box<str>),
    #[error("Cannot fetch {0}: {1}")]
    Download(Box<str>, reqwest::Error),
    /// what MikroTik answers at this address instead of a version
    #[error("{0} names no RouterOS version: {1}")]
    UnknownRelease(Box<str>, Box<str>),
    /// a `/system/resource` field, or that menu itself
    #[error("The device does not report {0}")]
    MissingDeviceState(&'static str),
    #[error("{0} is missing on the device after the upload")]
    UploadMissing(Box<str>),
    #[error("{file} on the device has {size} bytes, not {expected}")]
    UploadIncomplete {
        file: Box<str>,
        size: Box<str>,
        expected: usize,
    },
    /// the device runs another version or firmware than the update expected (a device without
    /// RouterBOARD has no firmware)
    #[error("Back after the reboot with RouterOS {version}, firmware {firmware:?}")]
    UnexpectedAfterReboot {
        version: Box<str>,
        firmware: Option<Box<str>>,
    },
    #[error("Not back after the reboot")]
    NotBackAfterReboot,
    /// the device switched back to the partition with the RouterOS from before the update
    #[error("The update was rolled back: the device runs RouterOS {version} from {partition}")]
    UpdateRolledBack {
        partition: Box<str>,
        version: Box<str>,
    },
    /// the OSPF neighbors `Full` before the update that aren't after it (`<instance> <router-id>`)
    #[error("OSPF neighbors missing after the update: {0}")]
    OspfNeighborsMissing(Box<str>),
    #[error("{0} runs a job already")]
    Busy(Box<str>),
    #[error("No job {0}")]
    UnknownJob(u32),
    #[error("Cancelled")]
    Cancelled,
    #[error("Ping: {0}")]
    Ping(#[from] surge_ping::SurgeError),
}

impl From<ResourceMutationError> for Error {
    fn from(value: ResourceMutationError) -> Self {
        Error::ResourceMutation(Box::new(value))
    }
}

impl From<MissingDependenciesError<'_, '_>> for Error {
    fn from(value: MissingDependenciesError) -> Self {
        Error::MissingDependenciesError(value.to_string().into_boxed_str())
    }
}
