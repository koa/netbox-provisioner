//! Scope: The issuer's signing keys: fetched at the start, again every hour and when a token
//! names a key not known, and handed to each request as its `Oidc`; not checking a token
//! (`main`).
//!
//! `actix-4-jwt-auth` keeps the keys it fetched once (`Oidc`, an `Arc<JWKSet>`), so after the
//! issuer changed its key every login was refused until a restart, and a key it withdrew
//! stayed valid. Here the keys are fetched anew and the `Oidc` of a request is the one of the
//! moment ([`with_issuer_keys`]); a token naming an unknown key fetches at once, at most every
//! [`UNKNOWN_KEY_PAUSE`], so made-up key ids don't flood the issuer.
use actix_4_jwt_auth::{
    Oidc, TokenLookup,
    biscuit::{Empty, JWT, jwk::JWKSet},
};
use actix_web::{
    Error,
    body::MessageBody,
    dev::{Extensions, ServiceRequest, ServiceResponse},
    http::header::AUTHORIZATION,
    middleware::Next,
    web::Data,
};
use log::{info, warn};
use serde::Deserialize;
use std::{
    collections::HashSet,
    rc::Rc,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use thiserror::Error;

/// How often the keys are fetched anyway.
const REFRESH: Duration = Duration::from_secs(60 * 60);
/// The least time between two fetches for tokens naming an unknown key.
const UNKNOWN_KEY_PAUSE: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub enum KeysError {
    #[error("fetching the issuer's keys: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the issuer's keys: {0}")]
    Oidc(#[from] actix_4_jwt_auth::OIDCValidationError),
}

/// The issuer's keys as last fetched.
pub struct IssuerKeys {
    issuer: String,
    client: reqwest::Client,
    state: RwLock<State>,
    /// when a token naming an unknown key last fetched them (the start and the hourly fetch
    /// don't count: right after a start a key the issuer had just changed waited a minute);
    /// held during that fetch, so the requests coming with the new key at once wait for it
    unknown_fetched: tokio::sync::Mutex<Option<Instant>>,
}

struct State {
    oidc: Oidc,
    /// the `kid`s of `oidc`'s keys
    known: HashSet<String>,
}

/// The part of the issuer's discovery document needed here.
#[derive(Deserialize)]
struct Discovery {
    jwks_uri: String,
}

impl IssuerKeys {
    /// Fetches the keys of `issuer` (its discovery document names where).
    pub async fn fetch(issuer: &str) -> Result<Arc<Self>, KeysError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;
        let issuer = issuer.trim_end_matches('/').to_string();
        let state = fetch_state(&client, &issuer).await?;
        Ok(Arc::new(Self {
            issuer,
            client,
            state: RwLock::new(state),
            unknown_fetched: tokio::sync::Mutex::new(None),
        }))
    }

    /// The keys as they are now.
    pub fn current(&self) -> Oidc {
        self.state.read().expect("keys lock").oidc.clone()
    }

    /// The keys to check `token` with: fetched anew first if it names a key not known and the
    /// last such fetch is long enough ago.
    pub async fn for_token(&self, token: &str) -> Oidc {
        let Some(kid) = key_id(token) else {
            return self.current();
        };
        if self.knows(&kid) {
            return self.current();
        }
        let mut last = self.unknown_fetched.lock().await;
        // another request may have fetched it meanwhile
        if should_fetch(
            &self.state.read().expect("keys lock").known,
            Some(&kid),
            last.map(|at| at.elapsed()),
        ) {
            // counts even when it fails: no second right after
            *last = Some(Instant::now());
            self.refresh("a token names a key not known").await;
        }
        self.current()
    }

    fn knows(&self, kid: &str) -> bool {
        self.state.read().expect("keys lock").known.contains(kid)
    }

    /// Fetches the keys anew; a failure keeps those there are.
    async fn refresh(&self, why: &str) {
        match fetch_state(&self.client, &self.issuer).await {
            Ok(state) => {
                let mut current = self.state.write().expect("keys lock");
                if state.known != current.known {
                    info!("issuer keys changed ({why}): {:?}", state.known);
                }
                *current = state;
            }
            Err(error) => warn!("issuer keys not fetched ({why}), keeping the old: {error}"),
        }
    }

    /// Fetches the keys every [`REFRESH`], for as long as the server runs.
    pub fn keep_fresh(self: Arc<Self>) {
        actix_web::rt::spawn(async move {
            loop {
                actix_web::rt::time::sleep(REFRESH).await;
                self.refresh("every hour").await;
            }
        });
    }
}

async fn fetch_state(client: &reqwest::Client, issuer: &str) -> Result<State, KeysError> {
    let discovery: Discovery = client
        .get(format!("{issuer}/.well-known/openid-configuration"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let jwks: JWKSet<Empty> = client
        .get(&discovery.jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let known = jwks
        .keys
        .iter()
        .filter_map(|key| key.common.key_id.clone())
        .collect();
    // `Oidc::new` would do the same, but its future isn't `Send` (`awc` in it)
    let oidc = Oidc::new_for_jwks(jwks, TokenLookup::Header(AUTHORIZATION.as_str().into()))?;
    Ok(State { oidc, known })
}

/// The `kid` in the header of `token`, read without checking the signature.
fn key_id(token: &str) -> Option<String> {
    JWT::<serde_json::Value, Empty>::new_encoded(token)
        .unverified_header()
        .ok()?
        .registered
        .key_id
}

/// Whether to fetch the keys for a token naming `kid`: it is not among the `known` ones, and the
/// last fetch for an unknown key was `since` ago, at least [`UNKNOWN_KEY_PAUSE`], or never. A
/// token without a `kid` fetches nothing (biscuit then tries the keys there are).
fn should_fetch(known: &HashSet<String>, kid: Option<&str>, since: Option<Duration>) -> bool {
    kid.is_some_and(|kid| !known.contains(kid)) && since.is_none_or(|s| s >= UNKNOWN_KEY_PAUSE)
}

/// Gives the request the keys of the moment as its `Oidc` (app data, where the extractor of
/// `actix-4-jwt-auth` looks). Goes first, before anything extracted from the request.
pub async fn with_issuer_keys(
    mut req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let keys = req
        .app_data::<Data<IssuerKeys>>()
        .cloned()
        .expect("IssuerKeys as app data");
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);
    let oidc = match token {
        Some(token) => keys.for_token(&token).await,
        None => keys.current(),
    };
    let mut data = Extensions::new();
    data.insert(oidc);
    req.add_data_container(Rc::new(data));
    next.call(req).await
}

#[cfg(test)]
mod test {
    use super::*;

    /// header `{"alg":"RS256","kid":"key-2"}`, payload `{}`, no signature
    const TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImtleS0yIn0.e30.";

    #[test]
    fn the_key_id_is_read_from_the_header() {
        assert_eq!(key_id(TOKEN).as_deref(), Some("key-2"));
        assert_eq!(key_id("not a token"), None);
    }

    #[test]
    fn only_an_unknown_key_fetches_and_not_too_often() {
        let known: HashSet<String> = ["key-1".to_string()].into();
        let long_ago = Some(UNKNOWN_KEY_PAUSE + Duration::from_secs(1));
        assert!(should_fetch(&known, Some("key-2"), long_ago));
        assert!(
            should_fetch(&known, Some("key-2"), None),
            "never fetched for one"
        );
        assert!(!should_fetch(&known, Some("key-1"), long_ago), "known");
        assert!(!should_fetch(&known, None, long_ago), "no kid");
        let just_before = Some(Duration::from_secs(5));
        assert!(
            !should_fetch(&known, Some("key-2"), just_before),
            "fetched just before"
        );
    }
}
