extern crate core;

use cynic::GraphQlError;
use cynic::http::CynicReqwestError;
use thiserror::Error;

mod config;
pub mod graphql;

#[derive(Debug, Error)]
pub enum NetboxError {
    #[error("accessing netbox API {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("errors from netbox {0:?}")]
    Graphql(#[from] CynicReqwestError),
    #[error("errors from netbox {0:?}")]
    ErrorFromServer(Box<[GraphQlError]>),
    #[error("cannot call netbox {0}")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),
    #[error("no data from netbox")]
    EmptyResult,
}
