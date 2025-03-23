use thiserror::Error;

pub mod config;
pub mod context;
pub mod device;
pub mod graphql;
pub mod netbox;
pub mod topology;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error accessing device")]
    MikrotikApi(#[from] mikrotik_api::error::Error),
    #[error("Error on data model")]
    MikrotikModel(#[from] mikrotik_model::resource::Error),
    #[error("Error credentials")]
    MissingCredentials,
}
