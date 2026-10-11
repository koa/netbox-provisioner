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
    /// On a VLAN group: VNI of the VXLAN its VLANs are tunneled over between
    /// devices (see `topology::VxlanData`).
    pub vxlan_vni: Option<u32>,
    /// On a wireless link: RouterOS `country` (5 GHz) or `region` (60 GHz) of both
    /// ends, overriding the one derived from the site's region.
    pub rf_country: Option<Box<str>>,
    /// On a wireless link: RouterOS `frequency-mode` of both ends (5 GHz).
    pub rf_frequency_mode: Option<Box<str>>,
}

/// NetBox's `Decimal` scalar (e.g. an interface's RF channel frequency in MHz),
/// sent as a string, but also accepted as a plain number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decimal(pub f64);

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(f64),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Number(value) => Ok(Decimal(value)),
            Raw::Text(text) => text.parse().map(Decimal).map_err(serde::de::Error::custom),
        }
    }
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

/// The `Authorization` header for a NetBox API token: a v2 token (NetBox 4.5+,
/// `nbt_<key>.<secret>`) goes as `Bearer`, a legacy v1 token (40 hex chars) as `Token`.
fn authorization_header(token: &str) -> String {
    if token.starts_with("nbt_") {
        format!("Bearer {token}")
    } else {
        format!("Token {token}")
    }
}

#[test]
fn test_authorization_header_by_token_version() {
    assert_eq!(authorization_header("nbt_abc.def"), "Bearer nbt_abc.def");
    assert_eq!(
        authorization_header("0123456789abcdef0123456789abcdef01234567"),
        "Token 0123456789abcdef0123456789abcdef01234567"
    );
}

fn netbox_client() -> Result<reqwest::Client, NetboxError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        authorization_header(CONFIG.netbox_token.as_str()).parse()?,
    );

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}
