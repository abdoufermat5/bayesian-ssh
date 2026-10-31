//! Connection handling: URL policy, the shared `reqwest` client, token cache and
//! the request plumbing every endpoint goes through.

use super::error::{ClientResult, TeamsError};
use crate::services::teams::crypto::DeviceKeys;
use bssh_teams_proto::api::{ApiError, TokenRequest};
use bssh_teams_proto::wire::{PublicKey32, Signature64};
use bssh_teams_proto::{API_PREFIX, CLIENT_VERSION_HEADER, PROTOCOL_HEADER, PROTOCOL_VERSION};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest long-poll a caller should ask for: under the request timeout.
pub const MAX_POLL_WAIT_SECS: u64 = 20;
/// Refresh the token this long before it expires.
const TOKEN_SKEW_SECS: i64 = 60;

/// Signs auth challenges. Implemented by [`DeviceKeys`]; tests use a fake.
pub trait AuthSigner: Send + Sync {
    fn sign_challenge(
        &self,
        challenge_id: Uuid,
        device_id: Uuid,
        nonce: &PublicKey32,
    ) -> Signature64;
}

impl AuthSigner for DeviceKeys {
    fn sign_challenge(
        &self,
        challenge_id: Uuid,
        device_id: Uuid,
        nonce: &PublicKey32,
    ) -> Signature64 {
        self.sign_auth_challenge(challenge_id, device_id, nonce)
    }
}

/// Check the server URL policy and return it normalised (no trailing slash).
pub fn validate_server_url(raw: &str) -> ClientResult<String> {
    let url = Url::parse(raw).map_err(|e| TeamsError::InvalidUrl(format!("{raw}: {e}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| TeamsError::InvalidUrl(format!("{raw}: no host")))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
        || host
            .trim_matches(|c| c == '[' || c == ']')
            .parse::<std::net::Ipv6Addr>()
            .is_ok_and(|ip| ip.is_loopback());
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => return Err(TeamsError::InsecureUrl(raw.to_string())),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(TeamsError::InvalidUrl(
            "credentials in the URL are not allowed".into(),
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

struct Token {
    value: String,
    expires_at: DateTime<Utc>,
}

pub struct TeamsClient {
    http: reqwest::Client,
    base: String,
    pub(super) device_id: Uuid,
    signer: Arc<dyn AuthSigner>,
    token: tokio::sync::Mutex<Option<Token>>,
}

impl TeamsClient {
    pub fn new(
        server_url: &str,
        device_id: Uuid,
        signer: Arc<dyn AuthSigner>,
    ) -> ClientResult<Self> {
        let base = validate_server_url(server_url)?;
        // Idempotent: Err just means a provider is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .tls_version_min(reqwest::tls::Version::TLS_1_3)
            .user_agent(format!("bayesian-ssh/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| TeamsError::Network(e.to_string()))?;
        Ok(Self {
            http,
            base,
            device_id,
            signer,
            token: tokio::sync::Mutex::new(None),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub(super) async fn send_raw(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&[u8]>,
        bearer: Option<&str>,
    ) -> ClientResult<(u16, Vec<u8>)> {
        let mut url = Url::parse(&format!("{}{}{}", self.base, API_PREFIX, path))
            .map_err(|e| TeamsError::InvalidUrl(e.to_string()))?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        let mut req = self
            .http
            .request(method, url)
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
            .header(CLIENT_VERSION_HEADER, env!("CARGO_PKG_VERSION"));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(b.to_vec());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| TeamsError::Network(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TeamsError::Network(e.without_url().to_string()))?;
        Ok((status, bytes.to_vec()))
    }

    pub(super) fn check(status: u16, bytes: Vec<u8>) -> ClientResult<Vec<u8>> {
        if (200..300).contains(&status) {
            return Ok(bytes);
        }
        match serde_json::from_slice::<ApiError>(&bytes) {
            Ok(error) => Err(TeamsError::Api { status, error }),
            Err(_) => Err(TeamsError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).chars().take(200).collect(),
            }),
        }
    }

    pub(super) async fn token(&self) -> ClientResult<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = guard.as_ref()
            && t.expires_at - ChronoDuration::seconds(TOKEN_SKEW_SECS) > Utc::now()
        {
            return Ok(t.value.clone());
        }
        let challenge = self.request_challenge().await?;
        let signature =
            self.signer
                .sign_challenge(challenge.challenge_id, self.device_id, &challenge.nonce);
        let resp = self
            .request_token(&TokenRequest {
                challenge_id: challenge.challenge_id,
                device_id: self.device_id,
                signature,
            })
            .await?;
        let value = resp.access_token.clone();
        *guard = Some(Token {
            value: resp.access_token,
            expires_at: resp.expires_at,
        });
        Ok(value)
    }

    pub(super) async fn invalidate_token(&self) {
        *self.token.lock().await = None;
    }

    /// Authenticated call; on 401 drops the cached token and retries once.
    pub(super) async fn call(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
    ) -> ClientResult<Vec<u8>> {
        let mut retried = false;
        loop {
            let bearer = self.token().await?;
            let (status, bytes) = self
                .send_raw(
                    method.clone(),
                    path,
                    query,
                    body.as_deref(),
                    Some(bearer.as_str()),
                )
                .await?;
            if status == 401 && !retried {
                retried = true;
                self.invalidate_token().await;
                continue;
            }
            return Self::check(status, bytes);
        }
    }

    pub(super) fn encode<B: Serialize>(body: &B) -> ClientResult<Vec<u8>> {
        serde_json::to_vec(body).map_err(|e| TeamsError::Decode(e.to_string()))
    }

    pub(super) fn decode<R: DeserializeOwned>(bytes: &[u8]) -> ClientResult<R> {
        serde_json::from_slice(bytes).map_err(|e| TeamsError::Decode(e.to_string()))
    }

    pub(super) async fn get<R: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> ClientResult<R> {
        Self::decode(&self.call(Method::GET, path, query, None).await?)
    }

    pub(super) async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> ClientResult<R> {
        let b = Self::encode(body)?;
        Self::decode(&self.call(Method::POST, path, &[], Some(b)).await?)
    }

    /// Unauthenticated POST (accounts and the auth handshake itself).
    pub(super) async fn post_plain<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> ClientResult<R> {
        let b = Self::encode(body)?;
        let (status, bytes) = self
            .send_raw(Method::POST, path, &[], Some(&b), None)
            .await?;
        Self::decode(&Self::check(status, bytes)?)
    }

    /// POST whose response body (if any) is ignored.
    pub(super) async fn post_unit<B: Serialize>(&self, path: &str, body: &B) -> ClientResult<()> {
        let b = Self::encode(body)?;
        self.call(Method::POST, path, &[], Some(b)).await?;
        Ok(())
    }

    // -- accounts, devices, auth ---------------------------------------------
}
