//! HTTP client for the bssh Teams server (`/v1`). One method per endpoint the
//! client needs; the sync engine uses it through [`TeamsTransport`].
//!
//! Security notes (PERIMETER.md, "network attacker"): https only (plain http
//! is accepted solely for loopback hosts, for development), TLS 1.3 minimum,
//! redirects are never followed (so a token can't be forwarded and https can't
//! be downgraded), and every request has a timeout.

use super::crypto::DeviceKeys;
use super::sync::TeamsTransport;
use async_trait::async_trait;
use bssh_teams_proto::api::*;
use bssh_teams_proto::wire::{PublicKey32, Signature64};
use bssh_teams_proto::{API_PREFIX, CLIENT_VERSION_HEADER, PROTOCOL_HEADER, PROTOCOL_VERSION};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Refresh the token this long before it expires.
const TOKEN_SKEW_SECS: i64 = 60;

#[derive(Debug, Error)]
pub enum TeamsError {
    #[error("refusing insecure server URL {0}: use https (http is only allowed for localhost)")]
    InsecureUrl(String),
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("server error {status}: {code:?}: {message}", code = .error.code, message = .error.message)]
    Api { status: u16, error: ApiError },
    #[error("unexpected response {status}: {body}")]
    Http { status: u16, body: String },
    #[error("could not decode server response: {0}")]
    Decode(String),
}

impl TeamsError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            TeamsError::Api { error, .. } => Some(error.code),
            _ => None,
        }
    }

    pub fn is_rotation_required(&self) -> bool {
        self.code() == Some(ErrorCode::RotationRequired)
    }

    /// True for failures worth retrying later (network, 5xx, rate limit).
    pub fn is_transient(&self) -> bool {
        match self {
            TeamsError::Network(_) => true,
            TeamsError::Http { status, .. } => *status >= 500,
            TeamsError::Api { error, .. } => {
                matches!(error.code, ErrorCode::RateLimited | ErrorCode::Internal)
            }
            _ => false,
        }
    }
}

pub type ClientResult<T> = std::result::Result<T, TeamsError>;

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
    device_id: Uuid,
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

    // -- plumbing ------------------------------------------------------------

    async fn send_raw(
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

    fn check(status: u16, bytes: Vec<u8>) -> ClientResult<Vec<u8>> {
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

    async fn token(&self) -> ClientResult<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = guard.as_ref() {
            if t.expires_at - ChronoDuration::seconds(TOKEN_SKEW_SECS) > Utc::now() {
                return Ok(t.value.clone());
            }
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

    async fn invalidate_token(&self) {
        *self.token.lock().await = None;
    }

    /// Authenticated call; on 401 drops the cached token and retries once.
    async fn call(
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

    fn encode<B: Serialize>(body: &B) -> ClientResult<Vec<u8>> {
        serde_json::to_vec(body).map_err(|e| TeamsError::Decode(e.to_string()))
    }

    fn decode<R: DeserializeOwned>(bytes: &[u8]) -> ClientResult<R> {
        serde_json::from_slice(bytes).map_err(|e| TeamsError::Decode(e.to_string()))
    }

    async fn get<R: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> ClientResult<R> {
        Self::decode(&self.call(Method::GET, path, query, None).await?)
    }

    async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> ClientResult<R> {
        let b = Self::encode(body)?;
        Self::decode(&self.call(Method::POST, path, &[], Some(b)).await?)
    }

    /// Unauthenticated POST (accounts and the auth handshake itself).
    async fn post_plain<B: Serialize, R: DeserializeOwned>(
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
    async fn post_unit<B: Serialize>(&self, path: &str, body: &B) -> ClientResult<()> {
        let b = Self::encode(body)?;
        self.call(Method::POST, path, &[], Some(b)).await?;
        Ok(())
    }

    // -- accounts, devices, auth ---------------------------------------------

    pub async fn create_account(
        &self,
        req: &CreateAccountRequest,
    ) -> ClientResult<CreateAccountResponse> {
        self.post_plain("/accounts", req).await
    }

    pub async fn add_device(&self, req: &AddDeviceRequest) -> ClientResult<AddDeviceResponse> {
        self.post("/devices", req).await
    }

    pub async fn request_challenge(&self) -> ClientResult<ChallengeResponse> {
        let req = ChallengeRequest {
            device_id: self.device_id,
        };
        self.post_plain("/auth/challenge", &req).await
    }

    pub async fn request_token(&self, req: &TokenRequest) -> ClientResult<TokenResponse> {
        self.post_plain("/auth/token", req).await
    }

    // -- teams, members, invites ---------------------------------------------

    /// The response type of `POST /teams` isn't specified in the contract; we
    /// assume [`TeamInfo`] (the caller needs the new `team_id`).
    pub async fn create_team(&self, req: &CreateTeamRequest) -> ClientResult<TeamInfo> {
        self.post("/teams", req).await
    }

    pub async fn list_teams(&self) -> ClientResult<Vec<TeamInfo>> {
        self.get("/teams", &[]).await
    }

    pub async fn get_team(&self, team_id: Uuid) -> ClientResult<TeamInfo> {
        self.get(&format!("/teams/{team_id}"), &[]).await
    }

    pub async fn list_members(&self, team_id: Uuid) -> ClientResult<Vec<Member>> {
        self.get(&format!("/teams/{team_id}/members"), &[]).await
    }

    pub async fn remove_member(&self, team_id: Uuid, member_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/teams/{team_id}/members/{member_id}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn create_invite(
        &self,
        team_id: Uuid,
        req: &CreateInviteRequest,
    ) -> ClientResult<Invite> {
        self.post(&format!("/teams/{team_id}/invites"), req).await
    }

    /// Join the team the invite is for; returns that team.
    pub async fn accept_invite(&self, req: &AcceptInviteRequest) -> ClientResult<TeamInfo> {
        self.post("/invites/accept", req).await
    }

    // -- billing -------------------------------------------------------------

    pub async fn billing(&self, team_id: Uuid) -> ClientResult<BillingInfo> {
        self.get(&format!("/teams/{team_id}/billing"), &[]).await
    }

    pub async fn billing_checkout(
        &self,
        team_id: Uuid,
        req: &CheckoutRequest,
    ) -> ClientResult<BillingRedirect> {
        self.post(&format!("/teams/{team_id}/billing/checkout"), req)
            .await
    }

    pub async fn billing_portal(&self, team_id: Uuid) -> ClientResult<BillingRedirect> {
        self.post(
            &format!("/teams/{team_id}/billing/portal"),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn pending_devices(&self, team_id: Uuid) -> ClientResult<PendingDevices> {
        self.get(&format!("/teams/{team_id}/devices/pending"), &[])
            .await
    }

    // -- keys ----------------------------------------------------------------

    /// The response body isn't specified; it is ignored.
    pub async fn put_keys(&self, team_id: Uuid, req: &PutKeysRequest) -> ClientResult<()> {
        self.post_unit(&format!("/teams/{team_id}/keys"), req).await
    }

    /// Every key version addressed to this device, oldest first.
    pub async fn get_keys(&self, team_id: Uuid) -> ClientResult<Vec<WrappedKey>> {
        self.get(&format!("/teams/{team_id}/keys"), &[]).await
    }

    // -- vault, audit --------------------------------------------------------

    pub async fn pull_vault(
        &self,
        team_id: Uuid,
        since: u64,
        limit: u32,
    ) -> ClientResult<PullResponse> {
        self.get(
            &format!("/teams/{team_id}/vault"),
            &[("since", since.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn push_vault(&self, team_id: Uuid, req: &PushRequest) -> ClientResult<PushResponse> {
        self.post(&format!("/teams/{team_id}/vault"), req).await
    }

    pub async fn ingest_audit_events(
        &self,
        team_id: Uuid,
        req: &IngestAuditRequest,
    ) -> ClientResult<IngestAuditResponse> {
        self.post(&format!("/teams/{team_id}/audit"), req).await
    }

    /// The team's audit log, newest first (admins only).
    pub async fn list_audit(
        &self,
        team_id: Uuid,
        filter: &AuditFilter,
    ) -> ClientResult<Vec<AuditRecord>> {
        let mut query = vec![("limit", filter.limit.unwrap_or(1000).to_string())];
        if let Some(device) = filter.device_id {
            query.push(("device_id", device.to_string()));
        }
        if let Some(entry) = filter.entry_id {
            query.push(("entry_id", entry.to_string()));
        }
        if let Some(since) = filter.since {
            query.push(("since", since.to_rfc3339()));
        }
        self.get(&format!("/teams/{team_id}/audit"), &query).await
    }
}

/// Filters for [`TeamsClient::list_audit`]. The server caps `limit` at 1000.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub device_id: Option<Uuid>,
    pub entry_id: Option<Uuid>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

#[async_trait(?Send)]
impl TeamsTransport for TeamsClient {
    async fn pull(&self, team_id: Uuid, since: u64, limit: u32) -> ClientResult<PullResponse> {
        self.pull_vault(team_id, since, limit).await
    }

    async fn push(&self, team_id: Uuid, req: PushRequest) -> ClientResult<PushResponse> {
        self.push_vault(team_id, &req).await
    }

    async fn ingest_audit(
        &self,
        team_id: Uuid,
        req: IngestAuditRequest,
    ) -> ClientResult<IngestAuditResponse> {
        self.ingest_audit_events(team_id, &req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::wire::Fixed;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct FakeSigner;
    impl AuthSigner for FakeSigner {
        fn sign_challenge(&self, _: Uuid, _: Uuid, _: &PublicKey32) -> Signature64 {
            Fixed([9u8; 64])
        }
    }

    #[test]
    fn url_policy() {
        for ok in [
            "https://teams.example.com",
            "https://teams.example.com/base/",
            "http://localhost:8080",
            "http://127.0.0.1:9000",
            "http://[::1]:9000",
        ] {
            assert!(validate_server_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://teams.example.com",
            "http://localhost.evil.com",
            "http://127.0.0.1.evil.com",
            "http://127.0.0.1@evil.com",
            "http://10.0.0.5",
            "ftp://localhost",
            "https://user:pw@teams.example.com",
            "not a url",
        ] {
            assert!(validate_server_url(bad).is_err(), "{bad}");
        }
        assert!(matches!(
            validate_server_url("http://teams.example.com"),
            Err(TeamsError::InsecureUrl(_))
        ));
    }

    #[test]
    fn client_refuses_plain_http_to_remote_host() {
        let r = TeamsClient::new(
            "http://teams.example.com",
            Uuid::new_v4(),
            Arc::new(FakeSigner),
        );
        assert!(matches!(r, Err(TeamsError::InsecureUrl(_))));
    }

    /// Minimal HTTP server: answers connection `i` with `respond(i)`.
    async fn mock_server<F>(respond: F) -> (String, Arc<Mutex<Vec<String>>>)
    where
        F: Fn(usize) -> (u16, String) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            let mut i = 0;
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let head_end = loop {
                    let n = sock.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break buf.len();
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let clen = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < head_end + clen {
                    let n = sock.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                seen2.lock().unwrap().push(head);
                let (status, body) = respond(i);
                i += 1;
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn challenge_json() -> String {
        serde_json::to_string(&ChallengeResponse {
            challenge_id: Uuid::new_v4(),
            nonce: Fixed([1u8; 32]),
            expires_at: Utc::now() + ChronoDuration::seconds(60),
        })
        .unwrap()
    }

    fn token_json(tok: &str, ttl_secs: i64) -> String {
        serde_json::to_string(&TokenResponse {
            access_token: tok.into(),
            expires_at: Utc::now() + ChronoDuration::seconds(ttl_secs),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn headers_auth_refresh_on_401_and_api_errors() {
        // Sequence: challenge, token(t1), GET -> 401, challenge, token(t2),
        // GET -> 200, GET -> 409 rotation_required.
        let rot = serde_json::to_string(&ApiError {
            code: ErrorCode::RotationRequired,
            message: "rotate".into(),
        })
        .unwrap();
        let (url, seen) = mock_server(move |i| match i {
            0 | 3 => (200, challenge_json()),
            1 => (200, token_json("t1", 3600)),
            2 => (
                401,
                "{\"code\":\"unauthorized\",\"message\":\"expired\"}".into(),
            ),
            4 => (200, token_json("t2", 3600)),
            5 => (200, "[]".to_string()),
            _ => (409, rot.clone()),
        })
        .await;
        let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

        assert!(client.list_teams().await.unwrap().is_empty());
        // Token cached: no new challenge for the next call, which errors typed.
        let err = client.get_team(Uuid::new_v4()).await.unwrap_err();
        assert!(err.is_rotation_required(), "{err}");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 7, "{seen:#?}");
        for head in seen.iter() {
            let lower = head.to_ascii_lowercase();
            assert!(lower.contains("bssh-protocol: 1"), "{head}");
            assert!(
                lower.contains(&format!(
                    "bssh-client-version: {}",
                    env!("CARGO_PKG_VERSION")
                )),
                "{head}"
            );
        }
        assert!(seen[2].to_ascii_lowercase().contains("bearer t1"));
        assert!(seen[5].to_ascii_lowercase().contains("bearer t2"));
        assert!(seen[6].to_ascii_lowercase().contains("bearer t2"));
        assert!(!seen[0].to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn token_near_expiry_is_refreshed() {
        let (url, seen) = mock_server(|i| match i {
            0 | 3 => (200, challenge_json()),
            1 => (200, token_json("short", 30)), // inside the skew window
            4 => (200, token_json("fresh", 3600)),
            _ => (200, "[]".into()),
        })
        .await;
        let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();
        client.list_teams().await.unwrap();
        client.list_teams().await.unwrap();
        let seen = seen.lock().unwrap();
        assert!(
            seen[5].to_ascii_lowercase().contains("bearer fresh"),
            "{seen:#?}"
        );
    }
}
