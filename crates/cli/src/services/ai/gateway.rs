//! bssh ai serve: MCP over streamable HTTP with bearer tokens, for unattended agents.
//!
//! The gateway runs one Teams agent identity (its credential, its admin-signed
//! policy, approvals relayed to admins). Clients present a short-lived token
//! from `bssh ai token`, signed with the agent's device key and scoped to that
//! agent and policy. Plain HTTP on localhost by default: put a reverse proxy
//! in front for TLS (nginx or caddy proxying to the loopback port).
//!
//! The MCP tools live in `mcp`; this file owns everything around them and mounts
//! them through the [`McpBackend`] seam (`mcp::RmcpBackend` in production).

use super::teams::{self, AgentContext, RelayBroker};
use super::types::{ApprovalBroker, Policy};
use crate::config::AppConfig;
use crate::services::teams::agent::{AgentCredential, AgentRuntime, VerifiedPolicy};
use crate::services::teams::crypto::{self, DeviceKeys};
use anyhow::{anyhow, bail, Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use bssh_teams_proto::wire::{Fixed, PublicKey32};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------- tokens

/// Signed over `TOKEN_DOMAIN ‖ payload JSON`, so a token signature can't be a signature for anything else.
const TOKEN_DOMAIN: &[u8] = b"bssh-ai/v1/gateway-token\0";
const TOKEN_PREFIX: &str = "bssh1";
pub const MAX_TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub v: u8,
    pub team_id: Uuid,
    pub agent: String,
    pub device_id: Uuid,
    /// The policy the token is scoped to.
    pub policy: String,
    /// Unix seconds.
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("malformed token")]
    Malformed,
    #[error("bad signature")]
    BadSignature,
    #[error("token expired")]
    Expired,
    #[error("token not valid yet")]
    NotYetValid,
    #[error("token is for another agent or policy")]
    WrongScope,
}

/// Parse `30s`, `15m`, `1h`, `2d`.
pub fn parse_ttl(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow!("invalid duration '{s}' (use 30s, 15m, 1h, 2d)"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => bail!("invalid duration '{s}' (use 30s, 15m, 1h, 2d)"),
    };
    let d = Duration::from_secs(secs);
    if d.is_zero() || d > MAX_TTL {
        bail!("a token lives from 1 second to 24 hours");
    }
    Ok(d)
}

/// Issue a token for the agent of `cred`, valid for `ttl` from `now` (unix seconds).
pub fn issue_token(cred: &AgentCredential, ttl: Duration, now: i64) -> Result<String> {
    if ttl.is_zero() || ttl > MAX_TTL {
        bail!("a token lives from 1 second to 24 hours");
    }
    let keys = cred.device_keys()?;
    sign_claims(
        &keys,
        &TokenClaims {
            v: 1,
            team_id: cred.team_id,
            agent: cred.agent_name.clone(),
            device_id: cred.device_id,
            policy: cred.policy.clone(),
            iat: now,
            exp: now + ttl.as_secs() as i64,
            jti: Uuid::new_v4(),
        },
    )
}

fn sign_claims(keys: &DeviceKeys, claims: &TokenClaims) -> Result<String> {
    let payload = serde_json::to_vec(claims)?;
    let mut msg = TOKEN_DOMAIN.to_vec();
    msg.extend_from_slice(&payload);
    let sig = crypto::sign_with(keys, &msg);
    Ok(format!(
        "{TOKEN_PREFIX}.{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(sig.as_bytes())
    ))
}

/// What a gateway accepts: tokens signed by this agent's key, for this agent and policy.
#[derive(Debug, Clone)]
pub struct TokenVerifier {
    pub agent_key: PublicKey32,
    pub team_id: Uuid,
    pub agent: String,
    pub device_id: Uuid,
    pub policy: String,
}

impl TokenVerifier {
    pub fn for_credential(cred: &AgentCredential) -> Result<Self> {
        Ok(Self {
            agent_key: cred.device_keys()?.ed25519_public(),
            team_id: cred.team_id,
            agent: cred.agent_name.clone(),
            device_id: cred.device_id,
            policy: cred.policy.clone(),
        })
    }

    /// Small clock skew is tolerated on `iat` only; `exp` is exact.
    pub fn verify(&self, token: &str, now: i64) -> std::result::Result<TokenClaims, TokenError> {
        let mut parts = token.split('.');
        let (Some(prefix), Some(p), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };
        if prefix != TOKEN_PREFIX {
            return Err(TokenError::Malformed);
        }
        let payload = URL_SAFE_NO_PAD
            .decode(p)
            .map_err(|_| TokenError::Malformed)?;
        let sig: [u8; 64] = URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| TokenError::Malformed)?
            .try_into()
            .map_err(|_| TokenError::Malformed)?;
        let mut msg = TOKEN_DOMAIN.to_vec();
        msg.extend_from_slice(&payload);
        if !crypto::verify_with(&self.agent_key, &msg, &Fixed(sig)) {
            return Err(TokenError::BadSignature);
        }
        let claims: TokenClaims =
            serde_json::from_slice(&payload).map_err(|_| TokenError::Malformed)?;
        if claims.v != 1 {
            return Err(TokenError::Malformed);
        }
        if claims.team_id != self.team_id
            || claims.device_id != self.device_id
            || claims.agent != self.agent
            || claims.policy != self.policy
        {
            return Err(TokenError::WrongScope);
        }
        if now >= claims.exp {
            return Err(TokenError::Expired);
        }
        if claims.iat > now + 60 || claims.exp - claims.iat > MAX_TTL.as_secs() as i64 {
            return Err(TokenError::NotYetValid);
        }
        Ok(claims)
    }
}

// ---------------------------------------------------------------- http

/// Shared by the auth middleware and the background watcher.
pub struct GatewayState {
    pub verifier: TokenVerifier,
    /// Set when the agent was removed or its team access is gone: every call is refused.
    pub revoked: AtomicBool,
}

impl GatewayState {
    pub fn new(verifier: TokenVerifier) -> Arc<Self> {
        Arc::new(Self {
            verifier,
            revoked: AtomicBool::new(false),
        })
    }
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    let mut r = (status, Json(serde_json::json!({ "error": msg }))).into_response();
    if status == StatusCode::UNAUTHORIZED {
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Bearer"
                .parse()
                .unwrap_or_else(|_| header::HeaderValue::from_static("Bearer")),
        );
    }
    r
}

async fn auth(State(state): State<Arc<GatewayState>>, mut req: Request, next: Next) -> Response {
    let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
    else {
        return json_error(StatusCode::UNAUTHORIZED, "missing bearer token");
    };
    let now = chrono::Utc::now().timestamp();
    match state.verifier.verify(token, now) {
        Ok(claims) => {
            if state.revoked.load(Ordering::Relaxed) {
                return json_error(StatusCode::FORBIDDEN, "this agent has been revoked");
            }
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(e) => {
            tracing::warn!("gateway: rejected a token: {e}");
            json_error(StatusCode::UNAUTHORIZED, &e.to_string())
        }
    }
}

/// `/healthz` is open; everything under `/mcp` needs a valid token.
pub fn router(state: Arc<GatewayState>, mcp: Router) -> Router {
    let protected = mcp.layer(middleware::from_fn_with_state(state, auth));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/mcp", protected)
}

// ---------------------------------------------------------------- MCP seam

/// What the MCP core needs from the gateway to build a `RunContext`: the
/// effective policy (admin-signed), the approval broker, the executor config.
pub struct GatewayContext {
    /// Config of the main database (hosts, local CA, runs store).
    pub config: AppConfig,
    pub team_id: Uuid,
    pub agent: String,
    pub policy_name: String,
    pub policy: Policy,
    pub policy_hash: String,
    pub broker: Arc<dyn ApprovalBroker>,
}

/// Mounts the MCP server for one gateway, under `/mcp` behind the token check.
/// Production uses `mcp::RmcpBackend` (rmcp's streamable HTTP service around
/// `mcp::build_server`); tests mount small fakes.
pub trait McpBackend: Send + Sync {
    fn router(&self, ctx: GatewayContext) -> Result<Router>;
}

/// A backend without MCP, for tests of the auth layer: answers 501 once a
/// request got through.
#[cfg(test)]
pub struct UnwiredMcp;

#[cfg(test)]
impl McpBackend for UnwiredMcp {
    fn router(&self, _ctx: GatewayContext) -> Result<Router> {
        Ok(Router::new().fallback(|| async {
            json_error(
                StatusCode::NOT_IMPLEMENTED,
                "the MCP server core is not wired into this build of the gateway",
            )
        }))
    }
}

// ---------------------------------------------------------------- serve

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub listen: String,
    /// `<team>/<agent>`.
    pub agent: String,
    /// Allow a non-loopback address (behind a TLS-terminating proxy or on a private network).
    pub allow_remote: bool,
}

/// How often the gateway re-syncs the vault, re-checks the policy and the agent's membership.
const WATCH_INTERVAL: Duration = Duration::from_secs(60);

pub fn check_listen(listen: &str, allow_remote: bool) -> Result<SocketAddr> {
    let addr: SocketAddr = listen
        .parse()
        .with_context(|| format!("invalid --listen '{listen}' (expected ip:port)"))?;
    if !addr.ip().is_loopback() && !allow_remote {
        bail!(
            "refusing to listen on {addr}: the gateway speaks plain HTTP. Keep it on localhost \
             and put a reverse proxy with TLS in front (nginx, caddy), or pass --allow-remote \
             if the network is trusted"
        );
    }
    Ok(addr)
}

/// The gateway's hosts all come from the team vault, whose tags any member can edit, so
/// they only widen the scope when `ai.trusted_tag_sources` names the team's source. A
/// policy scoped by tags would otherwise see no host at all: say so at startup.
fn untrusted_scope_tags_warning(
    policy: &Policy,
    team_source: Option<&str>,
    trusted_sources: &[String],
) -> Option<String> {
    let tags: Vec<&str> = policy
        .scope
        .tags
        .iter()
        .map(String::as_str)
        .filter(|t| *t != super::policy::ALL_HOSTS)
        .collect();
    if tags.is_empty() || team_source.is_some_and(|s| trusted_sources.iter().any(|t| t == s)) {
        return None;
    }
    let source = team_source.unwrap_or("<the team's source>");
    Some(format!(
        "the policy scopes hosts by tag ({}), but tags from source '{source}' are ignored for \
         scope: any team member can edit them. Scope by host name (scope.hosts), or add \
         '{source}' to ai.trusted_tag_sources if you trust every member's edits",
        tags.join(", ")
    ))
}

/// Run the gateway until Ctrl-C.
pub async fn serve(config: AppConfig, opts: ServeOptions, backend: &dyn McpBackend) -> Result<()> {
    let addr = check_listen(&opts.listen, opts.allow_remote)?;
    let cred = crate::services::teams::agent::find_credential(&config, &opts.agent)?;
    let runtime = Arc::new(AgentRuntime::new(cred)?);
    runtime
        .check_active()
        .await
        .context("checking the agent's team membership")?;
    let (agent_config, store) = runtime.open_store(&config)?;
    let refreshed = runtime
        .refresh(&config, &agent_config, &store)
        .await
        .context("syncing the team vault and the signed policy")?;
    teams::set_agent_context(Some(AgentContext {
        agent_config: agent_config.clone(),
        team_id: runtime.cred.team_id,
        agent_name: runtime.cred.agent_name.clone(),
    }));

    let team_source = crate::database::Database::new(&agent_config)
        .and_then(|db| crate::services::teams::session::local_teams(&db))
        .ok()
        .and_then(|teams| {
            teams
                .into_iter()
                .find(|(id, _)| *id == runtime.cred.team_id)
                .map(|(_, source)| source.name)
        });
    if let Some(warning) = untrusted_scope_tags_warning(
        &refreshed.policy.policy,
        team_source.as_deref(),
        &config.ai.trusted_tag_sources,
    ) {
        eprintln!("warning: {warning}");
    }

    let state = GatewayState::new(TokenVerifier::for_credential(&runtime.cred)?);
    let ctx = GatewayContext {
        config: config.clone(),
        team_id: runtime.cred.team_id,
        agent: runtime.cred.agent_name.clone(),
        policy_name: runtime.cred.policy.clone(),
        policy: refreshed.policy.policy.clone(),
        policy_hash: refreshed.policy.hash.clone(),
        broker: Arc::new(RelayBroker::from_runtime(&runtime, &config)),
    };
    let app = router(state.clone(), backend.router(ctx)?);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    println!(
        "bssh ai gateway for {} on http://{} ({} hosts, policy '{}' v{})",
        runtime.cred.id(),
        listener.local_addr()?,
        refreshed.hosts,
        runtime.cred.policy,
        watcher_version(&runtime, &config),
    );
    println!("Tokens: bssh ai token {} --ttl 1h", runtime.cred.id());
    // The watcher holds the (non-Send) Teams store, so it shares this task.
    let http = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    let result = tokio::select! {
        r = http => r.map_err(anyhow::Error::from),
        _ = watch(runtime.clone(), config.clone(), agent_config, state.clone(), refreshed.policy) => Ok(()),
    };
    teams::set_agent_context(None);
    result
}

fn watcher_version(runtime: &AgentRuntime, config: &AppConfig) -> u64 {
    crate::services::teams::agent::load_min_version(
        &crate::services::ai::ai_dir(config).join("agents"),
        &runtime.cred,
        &runtime.cred.policy,
    )
}

/// Revocation and policy watch: an admin removing the agent is the kill
/// switch, a re-signed policy needs a restart (logged), and the same sync
/// uploads the queued audit events.
async fn watch(
    runtime: Arc<AgentRuntime>,
    config: AppConfig,
    agent_config: AppConfig,
    state: Arc<GatewayState>,
    running: VerifiedPolicy,
) {
    // The store is not Send across awaits on every platform build: reopen per tick.
    loop {
        tokio::time::sleep(WATCH_INTERVAL).await;
        match runtime.check_active().await {
            Ok(()) => {}
            Err(e) => {
                let gone = e
                    .downcast_ref::<crate::services::teams::client::TeamsError>()
                    .is_some_and(|t| {
                        matches!(
                            t.code(),
                            Some(bssh_teams_proto::api::ErrorCode::Forbidden)
                                | Some(bssh_teams_proto::api::ErrorCode::Unauthorized)
                        )
                    })
                    || e.to_string().contains("no longer part of the team");
                if gone {
                    tracing::error!("agent revoked, refusing all calls: {e}");
                    state.revoked.store(true, Ordering::Relaxed);
                } else {
                    tracing::warn!("membership check failed: {e}");
                }
                continue;
            }
        }
        let Ok(store) = crate::services::teams::TeamsStore::open(&agent_config) else {
            continue;
        };
        match runtime.refresh(&config, &agent_config, &store).await {
            Ok(r) if r.policy.signed.version != running.signed.version => {
                tracing::warn!(
                    "policy re-signed (v{} -> v{}): restart the gateway to apply it",
                    running.signed.version,
                    r.policy.signed.version
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("gateway refresh failed: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::teams::relay::Peer;

    fn cred() -> AgentCredential {
        let agent = DeviceKeys::generate();
        let admin = DeviceKeys::generate();
        AgentCredential::new(
            "https://teams.example.com".into(),
            Uuid::new_v4(),
            "ops".into(),
            "deployer".into(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            &agent,
            admin.ed25519_public(),
            vec![Peer {
                device_id: Uuid::new_v4(),
                account_id: Uuid::new_v4(),
                label: "alice".into(),
                ed25519: admin.ed25519_public(),
                x25519: admin.x25519_public(),
            }],
            "default".into(),
        )
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn token_round_trip_and_expiry() {
        let c = cred();
        let v = TokenVerifier::for_credential(&c).unwrap();
        let t = issue_token(&c, Duration::from_secs(3600), NOW).unwrap();
        let claims = v.verify(&t, NOW + 10).unwrap();
        assert_eq!(claims.agent, "deployer");
        assert_eq!(claims.policy, "default");
        assert_eq!(claims.exp - claims.iat, 3600);
        // Exactly at exp and after: expired. Just before: fine.
        assert!(v.verify(&t, NOW + 3599).is_ok());
        assert_eq!(v.verify(&t, NOW + 3600), Err(TokenError::Expired));
        assert_eq!(v.verify(&t, NOW + 99_999), Err(TokenError::Expired));
        // From the future.
        assert_eq!(v.verify(&t, NOW - 3600), Err(TokenError::NotYetValid));
        // Two tokens differ (jti).
        assert_ne!(t, issue_token(&c, Duration::from_secs(3600), NOW).unwrap());
    }

    #[test]
    fn tampered_foreign_and_malformed_tokens_are_refused() {
        let c = cred();
        let v = TokenVerifier::for_credential(&c).unwrap();
        let t = issue_token(&c, Duration::from_secs(600), NOW).unwrap();

        // Payload swapped for a longer expiry: signature no longer matches.
        let parts: Vec<&str> = t.split('.').collect();
        let mut claims: TokenClaims =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        claims.exp += 10_000;
        let forged = format!(
            "{}.{}.{}",
            parts[0],
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
            parts[2]
        );
        assert_eq!(v.verify(&forged, NOW), Err(TokenError::BadSignature));

        // Signed by someone else's key.
        let other = cred();
        let theirs = issue_token(&other, Duration::from_secs(600), NOW).unwrap();
        assert_eq!(v.verify(&theirs, NOW), Err(TokenError::BadSignature));

        // Validly signed by the agent key but for another policy or agent.
        let keys = c.device_keys().unwrap();
        for tweak in [
            (|c: &mut TokenClaims| c.policy = "oncall".into()) as fn(&mut TokenClaims),
            |c| c.agent = "other".into(),
            |c| c.team_id = Uuid::new_v4(),
        ] {
            let mut cl: TokenClaims =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
            tweak(&mut cl);
            assert_eq!(
                v.verify(&sign_claims(&keys, &cl).unwrap(), NOW),
                Err(TokenError::WrongScope)
            );
        }

        for bad in ["", "x", "bssh1.a", "bssh1.a.b.c", "jwt.a.b", "bssh1.!!.!!"] {
            assert_eq!(v.verify(bad, NOW), Err(TokenError::Malformed), "{bad}");
        }
    }

    #[test]
    fn ttl_parsing_and_limits() {
        assert_eq!(parse_ttl("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_ttl("15m").unwrap(), Duration::from_secs(900));
        assert_eq!(parse_ttl("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_ttl("1d").unwrap(), Duration::from_secs(86_400));
        for bad in ["", "h", "0s", "2d", "10", "1w", "-5m"] {
            assert!(parse_ttl(bad).is_err(), "{bad}");
        }
        let c = cred();
        assert!(issue_token(&c, Duration::ZERO, NOW).is_err());
        assert!(issue_token(&c, Duration::from_secs(90_000), NOW).is_err());
    }

    #[test]
    fn listen_address_policy() {
        assert!(check_listen("127.0.0.1:8765", false).is_ok());
        assert!(check_listen("[::1]:8765", false).is_ok());
        assert!(check_listen("0.0.0.0:8765", false).is_err());
        assert!(check_listen("10.0.0.5:8765", true).is_ok());
        assert!(check_listen("nope", false).is_err());
    }

    async fn start(state: Arc<GatewayState>, backend: &dyn McpBackend) -> String {
        let ctx = GatewayContext {
            config: AppConfig::default(),
            team_id: state.verifier.team_id,
            agent: state.verifier.agent.clone(),
            policy_name: "default".into(),
            policy: Policy::default(),
            policy_hash: "h".into(),
            broker: Arc::new(NoBroker),
        };
        let app = router(state, backend.router(ctx).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    struct NoBroker;
    #[async_trait::async_trait]
    impl ApprovalBroker for NoBroker {
        async fn request(
            &self,
            _req: super::super::types::ApprovalRequest,
        ) -> Result<super::super::types::ApprovalOutcome> {
            Ok(super::super::types::ApprovalOutcome::Expired)
        }
    }

    /// A stand-in MCP service that echoes the verified agent from the request.
    struct EchoMcp;
    impl McpBackend for EchoMcp {
        fn router(&self, ctx: GatewayContext) -> Result<Router> {
            let agent = ctx.agent.clone();
            Ok(Router::new().route(
                "/ping",
                get(
                    move |axum::Extension(c): axum::Extension<TokenClaims>| async move {
                        format!("{agent}:{}", c.agent)
                    },
                ),
            ))
        }
    }

    #[tokio::test]
    async fn gateway_rejects_bad_tokens_and_serves_good_ones() {
        let c = cred();
        let state = GatewayState::new(TokenVerifier::for_credential(&c).unwrap());
        let base = start(state.clone(), &EchoMcp).await;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::new();
        let now = chrono::Utc::now().timestamp();

        // Health is open.
        assert_eq!(
            http.get(format!("{base}/healthz"))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );

        // No token, junk, wrong key, expired: 401.
        let get = |tok: Option<String>| {
            let mut r = http.get(format!("{base}/mcp/ping"));
            if let Some(t) = tok {
                r = r.bearer_auth(t);
            }
            r.send()
        };
        assert_eq!(get(None).await.unwrap().status(), 401);
        assert_eq!(get(Some("junk".into())).await.unwrap().status(), 401);
        let foreign = issue_token(&cred(), Duration::from_secs(60), now).unwrap();
        assert_eq!(get(Some(foreign)).await.unwrap().status(), 401);
        let expired = issue_token(&c, Duration::from_secs(60), now - 3600).unwrap();
        assert_eq!(get(Some(expired)).await.unwrap().status(), 401);

        // A good token reaches the MCP layer, which sees the verified claims.
        let good = issue_token(&c, Duration::from_secs(60), now).unwrap();
        let ok = get(Some(good.clone())).await.unwrap();
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.text().await.unwrap(), "deployer:deployer");

        // The kill switch refuses even a good token.
        state.revoked.store(true, Ordering::Relaxed);
        assert_eq!(get(Some(good)).await.unwrap().status(), 403);
    }

    #[tokio::test]
    async fn unwired_mcp_answers_501_after_auth() {
        let c = cred();
        let state = GatewayState::new(TokenVerifier::for_credential(&c).unwrap());
        let base = start(state, &UnwiredMcp).await;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::new();
        let anon = http.post(format!("{base}/mcp")).send().await.unwrap();
        assert_eq!(anon.status(), 401);
        let tok = issue_token(&c, Duration::from_secs(60), chrono::Utc::now().timestamp()).unwrap();
        let r = http
            .post(format!("{base}/mcp"))
            .bearer_auth(tok)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 501);
    }

    #[test]
    fn warns_when_scope_tags_come_from_an_untrusted_team_source() {
        let mut policy = Policy::default();
        assert!(untrusted_scope_tags_warning(&policy, Some("acme"), &[]).is_none());
        policy.scope.tags = vec![super::super::policy::ALL_HOSTS.into()];
        assert!(untrusted_scope_tags_warning(&policy, Some("acme"), &[]).is_none());
        policy.scope.tags.push("staging".into());
        let w = untrusted_scope_tags_warning(&policy, Some("acme"), &[]).expect("warning");
        assert!(w.contains("staging") && w.contains("'acme'"), "{w}");
        assert!(w.contains("ai.trusted_tag_sources"), "{w}");
        assert!(untrusted_scope_tags_warning(&policy, Some("acme"), &["acme".into()]).is_none());
        assert!(untrusted_scope_tags_warning(&policy, None, &["acme".into()]).is_some());
    }
}
