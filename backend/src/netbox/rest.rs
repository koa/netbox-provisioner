//! NetBox' REST API, for what GraphQL can't: reading the free addresses of a prefix and
//! writing (the helpers, `crate::helper`). Every write carries a `changelog_message`, so
//! NetBox' changelog tells which helper did it for whom although all go with one token.
use super::{NetboxError, authorization_header, web_url};
use crate::config::CONFIG;
use reqwest::{
    Method,
    header::{ACCEPT, AUTHORIZATION, HeaderMap},
};
use serde_json::{Map, Value};

pub struct NetboxRest {
    client: reqwest::Client,
    /// NetBox' API, ending in `api/`
    api: String,
    /// set on every write
    changelog_message: Option<Box<str>>,
}

impl NetboxRest {
    /// For reading, with `netbox_token`.
    pub fn reader() -> Result<Self, NetboxError> {
        Self::with_token(&CONFIG.netbox_token, None)
    }
    /// For writing, with `netbox_write_token`; only with `allow_netbox_write`.
    /// `changelog_message` goes into NetBox' changelog with every change.
    pub fn writer(changelog_message: impl Into<Box<str>>) -> Result<Self, NetboxError> {
        let token = CONFIG
            .netbox_write_token
            .as_deref()
            .filter(|_| CONFIG.allow_netbox_write)
            .ok_or(NetboxError::WriteNotAllowed)?;
        Self::with_token(token, Some(changelog_message.into()))
    }
    /// For the jobs' journal entries (`crate::journal`), with `netbox_write_token`, whatever
    /// `allow_netbox_write` says: that one lets the helpers write.
    pub fn journal(changelog_message: impl Into<Box<str>>) -> Result<Self, NetboxError> {
        let token = CONFIG.netbox_write_token.as_deref().ok_or(NetboxError::WriteNotAllowed)?;
        Self::with_token(token, Some(changelog_message.into()))
    }
    fn with_token(token: &str, changelog_message: Option<Box<str>>) -> Result<Self, NetboxError> {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization_header(token).parse()?);
        headers.insert(ACCEPT, "application/json".parse()?);
        Ok(Self {
            client: reqwest::Client::builder()
                .default_headers(headers)
                .build()?,
            api: format!("{}api/", web_url()),
            changelog_message,
        })
    }

    /// `path` below `api/`, e.g. `ipam/prefixes/1/available-ips/`.
    pub async fn get(&self, path: &str) -> Result<Value, NetboxError> {
        self.send(Method::GET, path, None).await
    }
    /// Creates an object, returns it as NetBox does (with its `id`).
    pub async fn create(&self, path: &str, object: Value) -> Result<Value, NetboxError> {
        self.send(Method::POST, path, Some(self.with_message(object)))
            .await
    }
    /// Changes the fields of `object` (`PATCH`), returns the object.
    pub async fn update(&self, path: &str, object: Value) -> Result<Value, NetboxError> {
        self.send(Method::PATCH, path, Some(self.with_message(object)))
            .await
    }
    pub async fn delete(&self, path: &str) -> Result<(), NetboxError> {
        self.send(Method::DELETE, path, Some(self.with_message(Value::Object(Map::new()))))
            .await
            .map(|_| ())
    }

    fn with_message(&self, mut object: Value) -> Value {
        if let (Some(message), Value::Object(fields)) = (&self.changelog_message, &mut object) {
            fields.insert("changelog_message".into(), Value::String(message.to_string()));
        }
        object
    }
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, NetboxError> {
        let mut request = self.client.request(method, format!("{}{path}", self.api));
        if let Some(body) = &body {
            request = request.json(body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(NetboxError::Rest {
                status,
                body: text.chars().take(500).collect::<String>().into(),
            });
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|error| NetboxError::UnexpectedResponse {
            status,
            error: error.to_string().into(),
            body: text.chars().take(300).collect::<String>().into(),
        })
    }
}
