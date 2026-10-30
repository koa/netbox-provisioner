use crate::config::CONFIG;
use async_graphql::{ComplexObject, SimpleObject};
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

pub mod rest;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/netbox/schema.graphqls",
    query_path = "src/netbox/fetch_topology.graphql",
    response_derives = "Debug, Clone, Serialize"
)]
pub struct FetchTopology;

/// NetBox' `BigInt` (an interface's `speed`), sent as a number or as a string.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BigInt {
    Number(i64),
    Text(String),
}

impl BigInt {
    /// The value as kbit/s, where it fits.
    pub fn kbits(&self) -> Option<u32> {
        match self {
            BigInt::Number(value) => u32::try_from(*value).ok(),
            BigInt::Text(value) => value.parse().ok(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// In a device's config context: the DNS servers its client networks get, the first
    /// asked first; the device doesn't resolve for them then.
    pub dns_servers: Option<Box<[IpAddr]>>,
    /// In a device's config context: how long (seconds) a new SSH connection from the
    /// internet keeps its source in a stage of the bruteforce protection (`docs/firewall.md`).
    pub ssh_stage_time: Option<u32>,
    /// In a device's config context: how long (seconds) a source the bruteforce protection
    /// caught stays blocked.
    pub ssh_blacklist_time: Option<u32>,
    /// On a device: its OSPF announces the default route it has (from DHCP or static);
    /// without, none leaves it.
    pub propagate_default_route: Option<bool>,
    /// On a prefix: the lease time of its DHCP server in seconds; without, RouterOS's 30 min.
    pub dhcp_lease_time: Option<u32>,
}

/// NetBox's `Decimal` scalar (e.g. an interface's RF channel frequency in MHz),
/// sent as a string, but also accepted as a plain number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decimal(pub f64);

impl Serialize for Decimal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

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
    #[error("netbox refused, HTTP {status}: {body}")]
    Rest {
        status: reqwest::StatusCode,
        body: Box<str>,
    },
    #[error("writing to netbox is disabled (allow_netbox_write, netbox_write_token)")]
    WriteNotAllowed,
    #[error("unexpected answer from netbox, HTTP {status} {error}: {body}")]
    UnexpectedResponse {
        status: reqwest::StatusCode,
        error: Box<str>,
        body: Box<str>,
    },
}

pub async fn fetch_topology() -> Result<fetch_topology::ResponseData, NetboxError> {
    let request_body = FetchTopology::build_query(fetch_topology::Variables {});
    let client = netbox_client()?;
    let response = client
        .post(netbox_url())
        .json(&request_body)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    // a proxy in front of NetBox may answer with text; show what came instead of JSON
    let unexpected = |error: Option<serde_json::Error>| NetboxError::UnexpectedResponse {
        status,
        error: error.map(|error| error.to_string()).unwrap_or_default().into(),
        body: body.chars().take(300).collect::<String>().into(),
    };
    if !status.is_success() {
        return Err(unexpected(None));
    }
    let response_body: Response<fetch_topology::ResponseData> =
        serde_json::from_str(&body).map_err(|error| unexpected(Some(error)))?;
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

/// NetBox' web UI, for links to its objects: `netbox_url` (the GraphQL endpoint) without
/// `graphql/`, ending in `/`.
pub fn web_url() -> String {
    web_url_of(netbox_url())
}

fn web_url_of(graphql_url: &str) -> String {
    let base = graphql_url.trim_end_matches('/');
    let base = base.strip_suffix("graphql").unwrap_or(base);
    format!("{}/", base.trim_end_matches('/'))
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

#[cfg(test)]
mod test {
    use super::web_url_of;

    #[test]
    fn web_url_is_the_graphql_url_without_graphql() {
        for url in [
            "http://127.0.0.1:8001/graphql/",
            "http://127.0.0.1:8001/graphql",
            "http://127.0.0.1:8001/",
        ] {
            assert_eq!(web_url_of(url), "http://127.0.0.1:8001/", "{url}");
        }
        assert_eq!(
            web_url_of("https://example.org/netbox/graphql/"),
            "https://example.org/netbox/"
        );
    }
}
