use crate::config::CONFIG;
use async_graphql::ComplexObject;
use async_graphql::SimpleObject;
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use thiserror::Error;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/netbox/schema.graphqls",
    query_path = "src/netbox/list-devices.graphql",
    response_derives = "Debug"
)]
struct ListDevices;
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

pub async fn list_devices() -> Result<Box<[Device]>, NetboxError> {
    let request_body = ListDevices::build_query(list_devices::Variables {});

    let client = netbox_client()?;
    let response_body: Response<list_devices::ResponseData> = client
        .post(netbox_url())
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(errors) = response_body.errors.filter(|data| !data.is_empty()) {
        Err(NetboxError::Graphql(errors.into()))
    } else if let Some(data) = response_body.data {
        let mut credentials_by_tenant = HashMap::new();
        for tenant in data.tenant_list {
            if let Some(credentials) = tenant.custom_field_data.mikrotik_credentials {
                credentials_by_tenant.insert(tenant.id, credentials);
            }
        }
        Ok(data
            .device_list
            .into_iter()
            .filter_map(|device| {
                let address = device
                    .primary_ip6
                    .and_then(|primary_ip| {
                        primary_ip
                            .address
                            .split_once('/')
                            .and_then(|(address, _)| Ipv6Addr::from_str(address).ok())
                            .map(|addr| IpAddr::V6(addr))
                    })
                    .or_else(|| {
                        device.primary_ip4.and_then(|primary_ip| {
                            primary_ip
                                .address
                                .split_once('/')
                                .and_then(|(address, _)| Ipv4Addr::from_str(address).ok())
                                .map(|addr| IpAddr::V4(addr))
                        })
                    });
                let credentials = device
                    .tenant
                    .and_then(|tenant| credentials_by_tenant.get(&tenant.id))
                    .or_else(|| {
                        device
                            .location
                            .and_then(|location| location.tenant)
                            .and_then(|tenant| credentials_by_tenant.get(&tenant.id))
                    })
                    .or_else(|| {
                        device
                            .site
                            .tenant
                            .and_then(|tenant| credentials_by_tenant.get(&tenant.id))
                    });
                if let (Some(name), Some(address), Some(credentials)) =
                    (device.name, address, credentials)
                {
                    Some(Device {
                        name: name.into_boxed_str(),
                        management_address: address,
                        credentials: credentials.clone(),
                    })
                } else {
                    None
                }
            })
            .collect())
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
