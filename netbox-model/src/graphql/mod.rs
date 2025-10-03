use crate::NetboxError;
use crate::config::CONFIG;
use crate::graphql::netbox::FetchTopology;
use crate::graphql::topology::Topology;
use cynic::QueryBuilder;
use cynic::http::ReqwestExt;
use reqwest::header::{AUTHORIZATION, HeaderMap};

pub mod netbox;
mod topology;

pub async fn fetch_topology() -> Result<Topology, NetboxError> {
    let client = netbox_client()?;
    let operation = FetchTopology::build(());
    let result = client.post(netbox_url()).run_graphql(operation).await?;
    if let Some(errors) = result.errors {
        Err(NetboxError::ErrorFromServer(errors.into_boxed_slice()))
    } else if let Some(data) = result.data {
        Ok(Topology::create(data))
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
