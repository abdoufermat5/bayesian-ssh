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
/// Longest long-poll a caller should ask for: under the request timeout.
pub const MAX_POLL_WAIT_SECS: u64 = 20;
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
    #[error("{}", api_text(*.status, .error))]
    Api { status: u16, error: ApiError },
    #[error("unexpected response {status}: {body}")]
    Http { status: u16, body: String },
    #[error("could not decode server response: {0}")]
    Decode(String),
    #[error("this server doesn't support organizations: it may need an update")]
    OrgUnsupported,
}

/// How an API error reads to the user. For billing and provider failures the
/// server's message already says what to do (seats, plan, "try again in a
/// minute"), so it is shown as is, without a status and code prefix.
fn api_text(status: u16, error: &ApiError) -> String {
    match error.code {
        ErrorCode::PaymentRequired | ErrorCode::SeatLimit | ErrorCode::Internal => {
            error.message.clone()
        }
        code => format!("server error {status}: {code:?}: {}", error.message),
    }
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

    /// The server wants proof that the account owns its email: request a code
    /// with [`TeamsClient::request_email_code`] and retry with it in
    /// [`CreateAccountRequest::email_code`].
    pub fn is_email_verification_required(&self) -> bool {
        self.code() == Some(ErrorCode::EmailVerificationRequired)
    }

    /// A mailed code was wrong, expired, already used or tried too often.
    pub fn is_invalid_email_code(&self) -> bool {
        self.code() == Some(ErrorCode::InvalidEmailCode)
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

/// Servers older than organizations answer `/orgs` with 404 (no route) or 405:
/// say so instead of showing the response body. The server's 404 may or may
/// not carry a JSON error, so both shapes map.
fn org_route(err: TeamsError) -> TeamsError {
    let missing = match &err {
        TeamsError::Http { status, .. } | TeamsError::Api { status, .. } => {
            matches!(*status, 404 | 405)
        }
        _ => false,
    };
    if missing {
        TeamsError::OrgUnsupported
    } else {
        err
    }
}

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

    /// Mail a one-time code that proves `email` (unauthenticated). The answer
    /// is the same whether or not a code was sent, so it says nothing about
    /// which addresses exist; see [`CreateAccountRequest::email_code`].
    pub async fn request_email_code(
        &self,
        req: &EmailCodeRequest,
    ) -> ClientResult<EmailCodeResponse> {
        self.post_plain("/accounts/email-code", req).await
    }

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

    /// Pending, unexpired invites (admin); tokens are never returned here.
    pub async fn list_invites(&self, team_id: Uuid) -> ClientResult<Vec<Invite>> {
        self.get(&format!("/teams/{team_id}/invites"), &[]).await
    }

    pub async fn revoke_invite(&self, team_id: Uuid, invite_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/teams/{team_id}/invites/{invite_id}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    /// Join the team the invite is for; returns that team.
    pub async fn accept_invite(&self, req: &AcceptInviteRequest) -> ClientResult<TeamInfo> {
        self.post("/invites/accept", req).await
    }

    // -- agents and the approval relay (bssh AI) -----------------------------

    /// Register an agent identity (admin). The agent takes a seat.
    pub async fn create_agent(
        &self,
        team_id: Uuid,
        req: &CreateAgentRequest,
    ) -> ClientResult<AgentInfo> {
        self.post(&format!("/teams/{team_id}/agents"), req).await
    }

    pub async fn list_agents(&self, team_id: Uuid) -> ClientResult<Vec<AgentInfo>> {
        self.get(&format!("/teams/{team_id}/agents"), &[]).await
    }

    /// Queue a relay message for its recipients.
    pub async fn post_relay(&self, team_id: Uuid, msg: &RelayMessage) -> ClientResult<RelayAck> {
        self.post(&format!("/teams/{team_id}/relay"), msg).await
    }

    /// Messages addressed to this device after `after`. Waits up to `wait_secs`
    /// (the server caps it at 30, this client's request timeout is 30 too, so
    /// callers ask for at most [`MAX_POLL_WAIT_SECS`]) for one to arrive.
    pub async fn poll_relay(
        &self,
        team_id: Uuid,
        kind: Option<RelayKind>,
        request_id: Option<Uuid>,
        after: u64,
        wait_secs: u64,
    ) -> ClientResult<RelayPoll> {
        let mut query = vec![
            ("after", after.to_string()),
            ("wait", wait_secs.min(MAX_POLL_WAIT_SECS).to_string()),
        ];
        match kind {
            Some(RelayKind::Request) => query.push(("kind", "request".into())),
            Some(RelayKind::Decision) => query.push(("kind", "decision".into())),
            None => {}
        }
        if let Some(id) = request_id {
            query.push(("request_id", id.to_string()));
        }
        self.get(&format!("/teams/{team_id}/relay"), &query).await
    }

    // -- organizations -------------------------------------------------------

    /// Organizations this account owns or has a team in.
    pub async fn list_orgs(&self) -> ClientResult<Vec<OrgInfo>> {
        self.get("/orgs", &[]).await.map_err(org_route)
    }

    /// A new empty organization owned by this account. Refused with
    /// `PaymentRequired` while the account owns another without a subscription.
    pub async fn create_org(&self) -> ClientResult<OrgInfo> {
        self.post("/orgs", &serde_json::json!({}))
            .await
            .map_err(org_route)
    }

    pub async fn get_org(&self, org_id: Uuid) -> ClientResult<OrgInfo> {
        self.get(&format!("/orgs/{org_id}"), &[])
            .await
            .map_err(org_route)
    }

    pub async fn add_org_owner(&self, org_id: Uuid, email: &str) -> ClientResult<OrgInfo> {
        self.post(
            &format!("/orgs/{org_id}/owners"),
            &AddOwnerRequest {
                email: email.to_string(),
            },
        )
        .await
        .map_err(org_route)
    }

    pub async fn remove_org_owner(&self, org_id: Uuid, account_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/orgs/{org_id}/owners/{account_id}"),
            &[],
            None,
        )
        .await
        .map_err(org_route)?;
        Ok(())
    }

    pub async fn org_billing(&self, org_id: Uuid) -> ClientResult<BillingInfo> {
        self.get(&format!("/orgs/{org_id}/billing"), &[])
            .await
            .map_err(org_route)
    }

    pub async fn org_billing_checkout(
        &self,
        org_id: Uuid,
        req: &CheckoutRequest,
    ) -> ClientResult<BillingRedirect> {
        self.post(&format!("/orgs/{org_id}/billing/checkout"), req)
            .await
            .map_err(org_route)
    }

    pub async fn org_billing_portal(&self, org_id: Uuid) -> ClientResult<BillingRedirect> {
        self.post(
            &format!("/orgs/{org_id}/billing/portal"),
            &serde_json::json!({}),
        )
        .await
        .map_err(org_route)
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
        if let Some(kind) = filter.kind {
            query.push(("kind", kind.as_str().to_string()));
        }
        if let Some(actor) = &filter.actor {
            query.push(("actor", actor.clone()));
        }
        self.get(&format!("/teams/{team_id}/audit"), &query).await
    }
}

/// Filters for [`TeamsClient::list_audit`]. The server caps `limit` at 1000.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub device_id: Option<Uuid>,
    pub entry_id: Option<Uuid>,
    pub kind: Option<AuditKind>,
    pub actor: Option<String>,
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

/// A tiny HTTP server for tests: answers connection `i` with `respond(i)`.
/// Every request is recorded as its head followed by its body, so tests can
/// check paths, headers and serialized bodies.
#[cfg(test)]
pub(crate) mod mock {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    pub(crate) async fn mock_server<F>(respond: F) -> (String, Arc<Mutex<Vec<String>>>)
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
                let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                seen2.lock().unwrap().push(format!("{head}{body}"));
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
}

#[cfg(test)]
mod tests {
    use super::mock::mock_server;
    use super::*;
    use bssh_teams_proto::wire::Fixed;

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

    #[test]
    fn org_routes_on_legacy_servers() {
        assert!(matches!(
            org_route(TeamsError::Http {
                status: 404,
                body: "no route".into()
            }),
            TeamsError::OrgUnsupported
        ));
        assert!(matches!(
            org_route(TeamsError::Http {
                status: 405,
                body: String::new()
            }),
            TeamsError::OrgUnsupported
        ));
        // A real API error (with its code) is kept: it says something useful.
        let api = TeamsError::Api {
            status: 403,
            error: ApiError {
                code: ErrorCode::Forbidden,
                message: "not an owner".into(),
            },
        };
        assert!(matches!(org_route(api), TeamsError::Api { .. }));
        // A JSON 404 (route missing behind a fallback) is the same as a bare one.
        let api_404 = TeamsError::Api {
            status: 404,
            error: ApiError {
                code: ErrorCode::NotFound,
                message: "no route".into(),
            },
        };
        assert!(matches!(org_route(api_404), TeamsError::OrgUnsupported));
    }

    #[tokio::test]
    async fn org_calls_use_org_routes() {
        let org = OrgInfo {
            org_id: Uuid::from_u128(7),
            my_role: OrgRole::Owner,
            owners: vec![OrgOwner {
                account_id: Uuid::from_u128(8),
                email: "ana@example.com".into(),
            }],
            team_ids: vec![],
            created_at: Utc::now(),
        };
        let org_json = serde_json::to_string(&org).unwrap();
        let owner_json = org_json.clone();
        let (url, seen) = mock_server(move |i| match i {
            0 => (200, challenge_json()),
            1 => (200, token_json("t", 3600)),
            2 => (200, format!("[{org_json}]")),
            3 => (200, owner_json.clone()),
            4 => (
                404,
                "{\"code\":\"not_found\",\"message\":\"no route\"}".into(),
            ),
            _ => (200, "{}".to_string()),
        })
        .await;
        let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

        assert_eq!(client.list_orgs().await.unwrap()[0].org_id, org.org_id);
        assert_eq!(
            client
                .add_org_owner(org.org_id, "bo@example.com")
                .await
                .unwrap(),
            org
        );
        let err = client.get_org(org.org_id).await.unwrap_err();
        assert!(matches!(err, TeamsError::OrgUnsupported), "{err}");

        let seen = seen.lock().unwrap();
        assert!(seen[2].starts_with("GET /v1/orgs HTTP/1.1"), "{}", seen[2]);
        assert!(
            seen[3]
                .starts_with("POST /v1/orgs/00000000-0000-0000-0000-000000000007/owners HTTP/1.1"),
            "{}",
            seen[3]
        );
        // The mock records headers only: the body's length proves the request.
        assert!(seen[3].contains("content-length: 26"), "{}", seen[3]);
        assert!(
            seen[4].starts_with("GET /v1/orgs/00000000-0000-0000-0000-000000000007 HTTP/1.1"),
            "{}",
            seen[4]
        );
    }

    #[test]
    fn billing_errors_show_the_server_message_as_is() {
        for code in [
            ErrorCode::PaymentRequired,
            ErrorCode::SeatLimit,
            ErrorCode::Internal,
        ] {
            let err = TeamsError::Api {
                status: 402,
                error: ApiError {
                    code,
                    message: "the team has no free seat; ask a team admin".into(),
                },
            };
            assert_eq!(
                err.to_string(),
                "the team has no free seat; ask a team admin"
            );
            assert_eq!(
                crate::services::teams::session::explain(&anyhow::Error::new(err)),
                None,
                "{code:?}: no hint replaces the server's text"
            );
        }
        let other = TeamsError::Api {
            status: 409,
            error: ApiError {
                code: ErrorCode::Conflict,
                message: "taken".into(),
            },
        };
        assert!(other.to_string().ends_with(": taken"), "{other}");
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

    #[tokio::test]
    async fn email_code_request_is_unauthenticated_json() {
        let answer = serde_json::to_string(&EmailCodeResponse {
            expires_in_secs: 900,
        })
        .unwrap();
        let (url, seen) = mock_server(move |_| (200, answer.clone())).await;
        let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

        let req = EmailCodeRequest {
            email: "ana@example.com".into(),
        };
        let resp = client.request_email_code(&req).await.unwrap();
        assert_eq!(resp.expires_in_secs, 900);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "one request, no auth handshake: {seen:#?}");
        assert!(
            seen[0].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
            "{}",
            seen[0]
        );
        assert!(!seen[0].to_ascii_lowercase().contains("authorization"));
        let body = serde_json::to_string(&req).unwrap();
        assert!(seen[0].contains(&body), "{}", seen[0]);
    }

    #[tokio::test]
    async fn email_verification_errors_keep_their_code() {
        let required = serde_json::to_string(&ApiError {
            code: ErrorCode::EmailVerificationRequired,
            message: "verify your email first".into(),
        })
        .unwrap();
        let invalid = serde_json::to_string(&ApiError {
            code: ErrorCode::InvalidEmailCode,
            message: "that code is wrong or expired".into(),
        })
        .unwrap();
        let (url, _seen) = mock_server(move |i| match i {
            0 => (403, required.clone()),
            _ => (403, invalid.clone()),
        })
        .await;
        let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();
        let req = CreateAccountRequest {
            email: "ana@example.com".into(),
            device: NewDevice {
                name: "laptop".into(),
                x25519_public: Fixed([1u8; 32]),
                ed25519_public: Fixed([2u8; 32]),
            },
            invite_token: None,
            email_code: None,
        };

        let err = client.create_account(&req).await.unwrap_err();
        assert!(err.is_email_verification_required(), "{err}");
        assert!(!err.is_invalid_email_code(), "{err}");
        assert_eq!(err.code(), Some(ErrorCode::EmailVerificationRequired));
        // Still distinguishable in what the user reads.
        assert!(
            err.to_string().contains("EmailVerificationRequired"),
            "{err}"
        );

        let err = client.create_account(&req).await.unwrap_err();
        assert!(err.is_invalid_email_code(), "{err}");
        assert!(!err.is_email_verification_required(), "{err}");
        assert!(err.to_string().contains("InvalidEmailCode"), "{err}");
    }
}
