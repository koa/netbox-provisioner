use crate::config::CONFIG;
use async_graphql::{ComplexObject, SimpleObject};
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/netbox/schema.graphqls",
    query_path = "src/netbox/fetch_topology.graphql",
    response_derives = "Debug"
)]
pub struct FetchTopology;

#[derive(Debug, Deserialize, Serialize)]
pub struct JSON {
    pub mikrotik_credentials: Option<Box<str>>,
    pub wlan_group: Option<u32>,
    pub wlan_mgmt: Option<u32>,
    pub controller: Option<u32>,
}

#[derive(Debug, SimpleObject)]
#[graphql(complex)]
pub struct Device {
    name: Box<str>,
    #[graphql(skip)]
    management_address: IpAddr,
    credentials: Box<str>,
}
#[ComplexObject]
impl Device {
    async fn management_address(&self) -> String {
        self.management_address.to_string()
    }
}

#[derive(Debug, Error)]
pub enum NetboxError {
    #[error("accessing netbox API {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("errors from netbox {0:?}")]
    Graphql(Box<[graphql_client::Error]>),
    #[error("cannot call netbox {0}")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),
    #[error("no data from netbox")]
    EmptyResult,
}

pub async fn fetch_topology() -> Result<fetch_topology::ResponseData, NetboxError> {
    let request_body = FetchTopology::build_query(fetch_topology::Variables {});
    let client = netbox_client()?;
    let response_body: Response<fetch_topology::ResponseData> = client
        .post(netbox_url())
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(errors) = response_body.errors.filter(|data| !data.is_empty()) {
        Err(NetboxError::Graphql(errors.into()))
    } else if let Some(data) = response_body.data {
        Ok(data)
    } else {
        Err(NetboxError::EmptyResult)
    }
}

fn netbox_url() -> &'static str {
    CONFIG.netbox_url.as_str()
}

fn netbox_client() -> Result<reqwest::Client, NetboxError> {
    let mut headers = HeaderMap::new();
    let access_token = CONFIG.netbox_token.as_str();
    headers.insert(AUTHORIZATION, format!("Token {access_token}").parse()?);

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}
