extern crate core;

use crate::device::ros::SetupError;
use mikrotik_model::resource::{MissingDependenciesError, ResourceMutationError};
use thiserror::Error;

pub mod config;
pub mod context;
pub mod device;
pub mod graphql;
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
    #[error("Missing required dependencies: {0}")]
    ErrorGeneratingString(#[from] std::fmt::Error),
    #[error("Cannot build setup {0}")]
    SetupError(#[from] SetupError),
}

impl From<MissingDependenciesError<'_, '_>> for Error {
    fn from(value: MissingDependenciesError) -> Self {
        Error::MissingDependenciesError(value.to_string().into_boxed_str())
    }
}
