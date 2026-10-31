//! `RunContext`: everything one agent run needs, and the guarded call pipeline.
//!
//! Pipeline for every call: ToolCall -> analyze -> decide -> audit `Call` -> approval when the
//! decision is Ask -> execute -> redact -> audit `Result` -> record output -> return.
//!
//! The context is transport-agnostic. Stdio builds one per process (`RunContext::from_config`);
//! the Teams gateway builds one per session with `RunContext::new`, passing its own
//! `ApprovalBroker` (the relay broker) and `Executor` through [`RunDeps`].
//!
//!

use super::files::{FileAccess, SftpFiles};
use super::server::McpElicitor;
use super::util::{
    normalize_path, now_rfc3339, parse_duration, path_matches, rfc3339_in, sha256_hex,
};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::approval::{serve_control, Elicitor, LocalBroker, RunInfo};
use crate::services::ai::audit::LocalAudit;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::plan::PlanStore;
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::redact::SecretRedactor;
use crate::services::ai::store::AiStore;
use crate::services::ai::types::*;
use crate::services::ai::{ai_dir, jail, report};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tracing::{debug, warn};

/// Why a tool call did not produce a result. Rendered to the agent as a tool error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// Policy, approval, budget or stop said no.
    Denied(String),
    /// Bad arguments.
    Invalid(String),
    /// Something broke (host unreachable, parse failure...).
    Failed(String),
    /// Something broke after the call had already run on the host (a parse failure, a
    /// partial change). The agent is told the command ran, not "Nothing ran.".
    FailedAfterRun(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Denied(m) => write!(f, "denied: {m}"),
            ToolError::Invalid(m) => write!(f, "invalid request: {m}"),
            ToolError::Failed(m) => write!(f, "failed: {m}"),
            ToolError::FailedAfterRun(m) => write!(f, "failed after running: {m}"),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError::Failed(format!("{e:#}"))
    }
}

pub type ToolResult = Result<Value, ToolError>;

/// What a tool call that is kept alive for an approval looks like from the outside.
#[derive(Debug, Clone)]
pub enum CallPhase {
    /// Still running in the foreground: no approval was needed yet.
    Running,
    /// The call is waiting for a human; `wait_approval` collects the result.
    Pending {
        request_id: String,
        expires_in_secs: u64,
        /// When the approval window closes.
        expires_at: Instant,
    },
    /// Finished (approved and executed, denied, or failed). Kept until the run ends.
    Done(ToolResult),
}

/// One in-flight tool call, shared by the agent's `wait_approval` and the task that runs it.
///
/// The handler runs exactly once, in a spawned task; approval only pauses it. That is what
/// makes repeated or concurrent `wait_approval` calls safe: they all observe the same result.
pub struct CallSlot {
    tool: String,
    tx: watch::Sender<CallPhase>,
}

impl CallSlot {
    pub(crate) fn new(tool: &str) -> Arc<Self> {
        let (tx, _) = watch::channel(CallPhase::Running);
        Arc::new(Self {
            tool: tool.to_string(),
            tx,
        })
    }

    pub fn tool(&self) -> &str {
        &self.tool
    }

    fn phase(&self) -> CallPhase {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<CallPhase> {
        self.tx.subscribe()
    }

    pub(crate) fn phase_now(&self) -> CallPhase {
        self.phase()
    }

    /// Mark the call pending unless it already is (or finished).
    fn mark_pending(&self, request_id: &str, expires_in_secs: u64) {
        self.tx.send_if_modified(|p| {
            if matches!(p, CallPhase::Running) {
                *p = CallPhase::Pending {
                    request_id: request_id.to_string(),
                    expires_in_secs,
                    expires_at: Instant::now() + Duration::from_secs(expires_in_secs),
                };
                true
            } else {
                false
            }
        });
    }

    pub(crate) fn finish(&self, result: ToolResult) {
        self.tx.send_replace(CallPhase::Done(result));
    }

    /// Wait for the call to finish, or for the deadline to pass.
    pub async fn wait_done(&self, until: Instant) -> Option<ToolResult> {
        let mut rx = self.tx.subscribe();
        loop {
            let phase = rx.borrow().clone();
            match phase {
                CallPhase::Done(r) => return Some(r),
                _ => {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    if tokio::time::timeout(left, rx.changed()).await.is_err() {
                        return None;
                    }
                }
            }
        }
    }

    pub fn pending(&self) -> Option<(String, u64, Instant)> {
        match self.phase() {
            CallPhase::Pending {
                request_id,
                expires_in_secs,
                expires_at,
            } => Some((request_id, expires_in_secs, expires_at)),
            _ => None,
        }
    }
}

tokio::task_local! {
    /// The slot of the tool call being served, so an approval can mark it pending.
    static CALL_SLOT: Arc<CallSlot>;
}

/// Serve `fut` as the handler of `slot`'s call.
pub(crate) async fn with_call_slot<F: Future>(slot: Arc<CallSlot>, fut: F) -> F::Output {
    CALL_SLOT.scope(slot, fut).await
}

pub(crate) fn current_call_slot() -> Option<Arc<CallSlot>> {
    CALL_SLOT.try_with(|s| s.clone()).ok()
}

/// Where the inventory comes from (the bssh database). A trait so tests use a fixed list.
#[expect(clippy::double_must_use, reason = "pending refactor")]
#[async_trait]
pub trait HostSource: Send + Sync {
    async fn list(&self) -> Result<Vec<Connection>>;
}

/// The real inventory.
pub struct DbHosts {
    config: AppConfig,
}

impl DbHosts {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl HostSource for DbHosts {
    async fn list(&self) -> Result<Vec<Connection>> {
        // Plain sync database read (SshService is not Sync, so keep it out of the future).
        crate::database::Database::new(&self.config)?.list_connections(None, false)
    }
}

/// Fixed inventory (tests, gateway previews).
pub struct StaticHosts(pub Vec<Connection>);

#[async_trait]
impl HostSource for StaticHosts {
    async fn list(&self) -> Result<Vec<Connection>> {
        Ok(self.0.clone())
    }
}

/// Visibility check: policy, connection, and whether that connection's tags may relax the
/// policy (`ai.trusted_tag_sources`). Most implementations are [`policy::host_in_scope`].
pub type ScopeFn = Arc<dyn Fn(&Policy, &Connection, bool) -> bool + Send + Sync>;

/// Everything a run calls out to. All injectable.
pub struct RunDeps {
    pub hosts: Arc<dyn HostSource>,
    pub files: Arc<dyn FileAccess>,
    pub executor: Arc<dyn Executor>,
    pub broker: Arc<dyn ApprovalBroker>,
    pub analyzer: Arc<dyn Analyzer>,
    pub redactor: Arc<dyn Redactor>,
    pub engine: Arc<dyn PolicyEngine>,
    pub audit: Arc<dyn AuditSink>,
    /// Is this connection visible to the agent? Default: `policy::host_in_scope`.
    pub scope: ScopeFn,
    /// MCP elicitation hook (stdio). The server stores the client's peer in it on initialize.
    pub elicitor: Option<Arc<McpElicitor>>,
}

struct AccessGrant {
    scope: Scope,
    expires: Instant,
}

/// A request waiting for a human, for `status`.
#[derive(Debug, Clone)]
struct PendingInfo {
    tier: Tier,
    summary: String,
    since: String,
}

/// Result of a guarded exec: redacted output and what was removed.
pub struct Executed {
    pub result: ExecResult,
    pub redactions: Vec<Redaction>,
}

pub struct RunContext {
    pub config: AppConfig,
    pub policy_name: String,
    pub policy: Policy,
    pub run_id: String,
    pub deps: RunDeps,
    pub plans: PlanStore,
    events: broadcast::Sender<AiEvent>,
    stop_tx: watch::Sender<bool>,
    stopped: AtomicBool,
    /// Paused by the circuit breaker. Different from `stopped`: a person can resume it.
    paused: AtomicBool,
    /// Why it paused, for `status` and the agent-facing error.
    pause_reason: Mutex<Option<String>>,
    /// Asynchronous approvals: a call that asks returns `pending` instead of blocking,
    /// and `wait_approval` collects the result. On for real MCP transports; off in tests
    /// that drive the broker synchronously.
    async_approvals: AtomicBool,
    /// Results of calls waiting for approval, by request id, kept until the run ends.
    approval_slots: Mutex<HashMap<String, Arc<CallSlot>>>,
    started: Instant,
    deadline: Instant,
    client: Mutex<String>,
    start_emitted: AtomicBool,
    /// The local broker behind the control socket, told the client name on `start`.
    local_broker: std::sync::OnceLock<Arc<LocalBroker>>,
    end_emitted: AtomicBool,
    calls: Mutex<VecDeque<Instant>>,
    denials: Mutex<VecDeque<Instant>>,
    facts: tokio::sync::Mutex<HashMap<String, Enforcement>>,
    /// Inventory source names by source id, resolved once for tag trust.
    source_names: Mutex<HashMap<uuid::Uuid, Option<String>>>,
    access: Mutex<Vec<AccessGrant>>,
    pending: Mutex<HashMap<String, PendingInfo>>,
    changes_used: AtomicU32,
}

impl RunContext {
    /// Build a context with a fresh run id.
    ///
    /// This is the seam for other transports: pass your own `deps.broker` / `deps.executor`.
    pub fn new(
        config: AppConfig,
        policy_name: String,
        policy: Policy,
        deps: RunDeps,
    ) -> Arc<RunContext> {
        Self::with_run_id(
            uuid::Uuid::new_v4().to_string(),
            config,
            policy_name,
            policy,
            deps,
        )
    }

    /// Same, with a run id chosen by the caller (the local broker needs it before the context).
    pub fn with_run_id(
        run_id: String,
        config: AppConfig,
        policy_name: String,
        policy: Policy,
        deps: RunDeps,
    ) -> Arc<RunContext> {
        let ttl = parse_duration(&policy.budgets.run_ttl).unwrap_or_else(|e| {
            warn!(
                "budgets.run_ttl '{}': {e}; using 2h",
                policy.budgets.run_ttl
            );
            Duration::from_secs(2 * 3600)
        });
        let now = Instant::now();
        let (events, _) = broadcast::channel(256);
        let (stop_tx, _) = watch::channel(false);
        Arc::new(RunContext {
            config,
            policy_name,
            policy,
            run_id,
            deps,
            plans: PlanStore::default(),
            events,
            stop_tx,
            stopped: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            pause_reason: Mutex::new(None),
            async_approvals: AtomicBool::new(false),
            approval_slots: Mutex::new(HashMap::new()),
            started: now,
            deadline: now + ttl,
            client: Mutex::new("unknown".into()),
            start_emitted: AtomicBool::new(false),
            local_broker: std::sync::OnceLock::new(),
            end_emitted: AtomicBool::new(false),
            calls: Mutex::new(VecDeque::new()),
            denials: Mutex::new(VecDeque::new()),
            facts: tokio::sync::Mutex::new(HashMap::new()),
            source_names: Mutex::new(HashMap::new()),
            access: Mutex::new(Vec::new()),
            pending: Mutex::new(HashMap::new()),
            changes_used: AtomicU32::new(0),
        })
    }

    /// The local, single-user wiring used by `bssh ai mcp`: real store, engine, analyzer,
    /// redactor, audit, executor, `LocalBroker` with MCP elicitation, and the control socket.
    pub async fn from_config(config: AppConfig, policy_name: &str) -> Result<Arc<RunContext>> {
        let policy = policy::resolve(&config, policy_name)?;
        let run_id = uuid::Uuid::new_v4().to_string();
        let store = Arc::new(AiStore::open(&config)?);
        let audit: Arc<dyn AuditSink> = Arc::new(LocalAudit::open(&config)?);
        // The repo stores no connection passwords in the vault, so there is nothing exact to
        // match; pattern-based redaction still applies.
        let redactor: Arc<dyn Redactor> = Arc::new(SecretRedactor::new(Vec::new(), &policy.redact));
        let elicitor = Arc::new(McpElicitor::default());
        let info = RunInfo {
            run_id: run_id.clone(),
            client: "unknown".into(),
            policy: policy_name.to_string(),
            started_at: now_rfc3339(),
            on_behalf_of: whoami::username(),
            hosts: Vec::new(),
        };
        let broker = Arc::new(LocalBroker::new(
            store.clone(),
            info.clone(),
            Some(elicitor.clone() as Arc<dyn Elicitor>),
            config.ai.approval_timeout_secs,
        ));
        let deps = RunDeps {
            hosts: Arc::new(DbHosts::new(&config)),
            files: Arc::new(SftpFiles::new(&config)),
            executor: Arc::new(
                AiExecutor::new(&config)
                    .with_read_paths(policy.read_paths.clone())
                    .with_deny_paths(policy.deny_paths.clone()),
            ),
            broker: broker.clone(),
            analyzer: Arc::new(ShellAnalyzer::new()),
            redactor,
            engine: Arc::new(DefaultPolicyEngine::new(store)),
            audit,
            scope: Arc::new(policy::host_in_scope),
            elicitor: Some(elicitor),
        };
        let ctx = Self::with_run_id(run_id, config, policy_name.to_string(), policy, deps);
        ctx.enable_async_approvals();
        let mut info = info;
        info.hosts = ctx
            .visible_hosts()
            .await
            .map(|hosts| hosts.into_iter().map(|c| c.name).collect())
            .unwrap_or_default();
        ctx.spawn_control(broker, info);
        Ok(ctx)
    }

    /// Serve `<ai_dir>/runs/<run_id>.sock` for the desktop app and `bssh ai approve/stop`.
    /// A control-socket failure is logged and the run continues without it (approvals then
    /// only work in-client), it never stops the run.
    pub fn spawn_control(self: &Arc<Self>, broker: Arc<LocalBroker>, info: RunInfo) {
        let _ = self.local_broker.set(broker.clone());
        let ai = ai_dir(&self.config);
        if let Err(e) = crate::services::ai::control_client::ensure_runs_dir(&ai) {
            warn!("control socket dir: {e:#}");
            return;
        }
        let socket = crate::services::ai::control_client::socket_path(&ai, &self.run_id);
        let ctx = self.clone();
        let events = self.events.clone();
        let stop_rx = self.stop_tx.subscribe();
        let resume_ctx = self.clone();
        let resume: crate::services::ai::approval::ResumeFn =
            Arc::new(move |by: &str| resume_ctx.resume(by));
        tokio::spawn(async move {
            match serve_control(socket, info, broker, events, stop_rx, resume).await {
                // A client sent Stop.
                Ok(Some(reason)) => {
                    if !ctx.is_stopped() {
                        ctx.stop(&format!("stopped from the control socket: {reason}"));
                    }
                }
                // The run's own stop flag fired: nothing left to do.
                Ok(None) => {}
                Err(e) => warn!("control socket: {e:#}"),
            }
        });
    }

    // ------------------------------------------------------------ run lifecycle

    pub fn subscribe(&self) -> broadcast::Receiver<AiEvent> {
        self.events.subscribe()
    }

    pub fn events(&self) -> broadcast::Sender<AiEvent> {
        self.events.clone()
    }

    pub fn client(&self) -> String {
        self.client.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Record the self-reported client and emit `RunStarted` (once).
    pub fn start(&self, client: &str) {
        if let Ok(mut c) = self.client.lock() {
            *c = client.to_string();
        }
        if let Some(b) = self.local_broker.get() {
            b.set_client(client);
        }
        if self.start_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        let policy_json = serde_json::to_string(&self.policy).unwrap_or_default();
        let ev = AiEvent::RunStarted {
            run_id: self.run_id.clone(),
            client: client.to_string(),
            on_behalf_of: whoami::username(),
            policy: self.policy_name.clone(),
            policy_hash: sha256_hex(policy_json.as_bytes()),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit RunStarted: {e:#}");
        }
    }

    /// Emit `RunEnded` once.
    pub fn end(&self, reason: &str) {
        if self.end_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        let ev = AiEvent::RunEnded {
            run_id: self.run_id.clone(),
            reason: reason.to_string(),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit RunEnded: {e:#}");
        }
    }

    /// Kill switch: no more calls succeed, the transport shuts down.
    pub fn stop(&self, reason: &str) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stop_tx.send_replace(true);
        self.end(reason);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Resolves when the run is stopped (kill switch, breaker, TTL).
    pub async fn stopped(&self) {
        let mut rx = self.stop_tx.subscribe();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    pub fn emit(&self, ev: &AiEvent) -> Result<()> {
        let res = self.deps.audit.record(ev);
        // No subscribers is normal.
        let _ = self.events.send(ev.clone());
        res
    }

    /// Run still allowed to make calls?
    pub fn check_alive(&self) -> Result<(), ToolError> {
        if self.is_stopped() {
            return Err(ToolError::Denied(
                "this run was stopped; ask the user to start a new agent session".into(),
            ));
        }
        if self.is_paused() {
            let reason = self
                .pause_reason
                .lock()
                .ok()
                .and_then(|r| r.clone())
                .unwrap_or_else(|| "too many denials in a minute".into());
            return Err(ToolError::Denied(format!(
                "{reason}; the user can resume it with `bssh ai resume {}` or in the desktop app",
                self.short_run_id()
            )));
        }
        if Instant::now() >= self.deadline {
            self.stop("run TTL reached");
            return Err(ToolError::Denied(
                "this run has expired (TTL) and stopped; ask the user to restart the agent".into(),
            ));
        }
        Ok(())
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// First 8 characters of the run id, as `bssh ai status` shows it.
    pub fn short_run_id(&self) -> &str {
        self.run_id.get(..8).unwrap_or(&self.run_id)
    }

    /// Pause the run after the circuit breaker tripped. Calls are refused (except `status`
    /// and `wait_approval`) until [`resume`](Self::resume); the run is not stopped.
    fn pause(&self, reason: String) {
        if self.paused.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut r) = self.pause_reason.lock() {
            *r = Some(reason.clone());
        }
        let ev = AiEvent::Paused {
            run_id: self.run_id.clone(),
            reason,
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Paused: {e:#}");
        }
    }

    /// Clear a circuit-breaker pause and the denial window. `Err` when the run was stopped
    /// (stop is final) or was not paused.
    pub fn resume(&self, by: &str) -> Result<(), String> {
        if self.is_stopped() {
            return Err(
                "the run is stopped and cannot be resumed; start a new agent session".into(),
            );
        }
        if !self.paused.swap(false, Ordering::SeqCst) {
            return Err("the run is not paused".into());
        }
        if let Ok(mut r) = self.pause_reason.lock() {
            *r = None;
        }
        // A resume starts a fresh window: the denials that tripped it don't count again.
        if let Ok(mut d) = self.denials.lock() {
            d.clear();
        }
        let ev = AiEvent::Resumed {
            run_id: self.run_id.clone(),
            by: by.to_string(),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Resumed: {e:#}");
        }
        Ok(())
    }

    /// Turn on asynchronous approvals: a call that needs a human returns `pending` and
    /// `wait_approval` collects the result. Real MCP transports use this; tests can drive
    /// the broker synchronously.
    pub fn enable_async_approvals(&self) {
        self.async_approvals.store(true, Ordering::SeqCst);
    }

    pub fn async_approvals(&self) -> bool {
        self.async_approvals.load(Ordering::SeqCst)
    }

    /// The call waiting on `request_id`, for `wait_approval`.
    pub fn pending_slot(&self, request_id: &str) -> Option<Arc<CallSlot>> {
        self.approval_slots
            .lock()
            .ok()
            .and_then(|m| m.get(request_id).cloned())
    }

    /// Register a call as waiting for a human and tell its slot.
    fn mark_pending(&self, request_id: &str, expires_in_secs: u64) {
        if !self.async_approvals() {
            return;
        }
        let Some(slot) = current_call_slot() else {
            return;
        };
        slot.mark_pending(request_id, expires_in_secs);
        if let Ok(mut m) = self.approval_slots.lock() {
            m.insert(request_id.to_string(), slot);
        }
    }

    /// `calls_per_min` sliding window.
    pub fn rate_limit(&self) -> Result<(), ToolError> {
        let limit = self.policy.budgets.calls_per_min as usize;
        let now = Instant::now();
        let over = {
            let mut calls = self.calls.lock().map_err(|_| poisoned())?;
            while calls
                .front()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
            {
                calls.pop_front();
            }
            if limit > 0 && calls.len() >= limit {
                true
            } else {
                calls.push_back(now);
                false
            }
        };
        if over {
            return Err(ToolError::Denied(format!(
                "call budget exceeded ({limit} calls per minute); wait and retry"
            )));
        }
        Ok(())
    }

    /// Count a denial by the policy engine of an agent-initiated action. A burst pauses the
    /// run. Human decisions, expired approvals, argument mistakes, budget limits and the
    /// observe-profile refusal do not count.
    pub fn note_denial(&self, reason: &str) {
        if !counts_for_breaker(reason) {
            return;
        }
        let max = self.policy.budgets.max_denials_per_min as usize;
        let now = Instant::now();
        let tripped = {
            let Ok(mut d) = self.denials.lock() else {
                return;
            };
            d.push_back(now);
            while d
                .front()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
            {
                d.pop_front();
            }
            max > 0 && d.len() >= max
        };
        debug!("denial: {reason}");
        if tripped {
            self.pause(format!(
                "paused by the circuit breaker after {max} denials in a minute"
            ));
        }
    }

    pub fn consume_change(&self) -> Result<(), ToolError> {
        let max = self.policy.budgets.changes_per_run;
        let prev = self.changes_used.fetch_add(1, Ordering::SeqCst);
        if prev >= max {
            self.changes_used.fetch_sub(1, Ordering::SeqCst);
            return Err(ToolError::Denied(format!(
                "change budget exhausted ({max} per run)"
            )));
        }
        Ok(())
    }

    // ------------------------------------------------------------ hosts

    pub fn in_scope(&self, conn: &Connection) -> bool {
        if (self.deps.scope)(&self.policy, conn, self.tags_trusted(conn)) {
            return true;
        }
        let now = Instant::now();
        let Ok(mut grants) = self.access.lock() else {
            return false;
        };
        grants.retain(|g| g.expires > now);
        grants.iter().any(|g| {
            g.scope.hosts.iter().any(|h| h == &conn.name)
                || g.scope
                    .tags
                    .iter()
                    .any(|t| conn.tags.iter().any(|ct| ct == t))
        })
    }

    pub async fn visible_hosts(&self) -> Result<Vec<Connection>, ToolError> {
        let all = self.deps.hosts.list().await?;
        Ok(all.into_iter().filter(|c| self.in_scope(c)).collect())
    }

    /// Host by name or alias, only when in scope. Unknown and out-of-scope look the same to
    /// the agent; argument mistakes and probes never trip the circuit breaker.
    pub async fn resolve_host(&self, name: &str) -> Result<Connection, ToolError> {
        let all = self.deps.hosts.list().await?;
        let found = all
            .into_iter()
            .find(|c| c.name == name || c.aliases.iter().any(|a| a == name));
        match found {
            Some(c) if self.in_scope(&c) => Ok(c),
            _ => Err(ToolError::Invalid(format!(
                "unknown host '{name}' (use list_hosts to see the hosts you can reach)"
            ))),
        }
    }

    /// Grant wider scope for a while (after an approved `request_access`).
    pub fn grant_access(&self, scope: Scope, duration: Duration) {
        if let Ok(mut g) = self.access.lock() {
            g.push(AccessGrant {
                scope,
                expires: Instant::now() + duration,
            });
        }
    }

    /// Enforcement facts for a host, probed once per run.
    ///
    /// A probe that failed is *not* cached: caching "no jail, no certificates" would
    /// downgrade the host for the rest of the run after one network hiccup. The error is
    /// reported in [`HostFacts::probe_error`] and the next call tries again.
    pub async fn facts(&self, conn: &Connection) -> HostFacts {
        let cached = self.facts.lock().await.get(&conn.name).cloned();
        let (enforcement, probe_error) = match cached {
            Some(e) => (e, None),
            None => {
                let mut errors: Vec<String> = Vec::new();
                let interactive_bastion = conn.use_kerberos && conn.bastion.is_some();
                // One connection for the jail trials and the agent check: through a
                // bastion each exec is another login.
                let (jail_kind, agent_user) =
                    match jail::probe_with_agent(conn, self.deps.executor.as_ref()).await {
                        Ok(found) => found,
                        Err(e) => {
                            errors.push(format!("probe: {e:#}"));
                            (JailKind::None, false)
                        }
                    };
                // A certificate makes the target login bssh-agent, which cannot pass
                // through a bastion that logs in as the user: never promise it there.
                let e = Enforcement {
                    jail: jail_kind,
                    certificates: !interactive_bastion
                        && crate::services::ai::cert::ca_available(&self.config)
                        && agent_user,
                    agent_user,
                };
                let error = if errors.is_empty() {
                    self.facts.lock().await.insert(conn.name.clone(), e.clone());
                    None
                } else {
                    Some(errors.join("; "))
                };
                (e, error)
            }
        };
        let source = self.source_name(conn);
        HostFacts {
            name: conn.name.clone(),
            tags: conn.tags.clone(),
            tags_trusted: policy::tags_trusted(
                &self.config.ai.trusted_tag_sources,
                conn,
                source.as_deref(),
            ),
            tag_source: source,
            enforcement,
            probe_error,
        }
    }

    /// The inventory source a connection came from, resolved once per source id.
    fn source_name(&self, conn: &Connection) -> Option<String> {
        let id = conn.source_id?;
        if let Ok(cache) = self.source_names.lock() {
            if let Some(name) = cache.get(&id) {
                return name.clone();
            }
        }
        let name = crate::services::ai::source_name(&self.config, conn);
        if let Ok(mut cache) = self.source_names.lock() {
            cache.insert(id, name.clone());
        }
        name
    }

    /// May this connection's tags relax the policy? Sources other than bssh's own
    /// connections count only when `ai.trusted_tag_sources` names them.
    fn tags_trusted(&self, conn: &Connection) -> bool {
        let source = self.source_name(conn);
        policy::tags_trusted(&self.config.ai.trusted_tag_sources, conn, source.as_deref())
    }

    /// Set a host's enforcement without probing it (gateway-side knowledge, tests).
    pub async fn prime_enforcement(&self, host: &str, e: Enforcement) {
        self.facts.lock().await.insert(host.to_string(), e);
    }

    /// Mode for read-only work: the host's jail, run as the low-privilege agent principal
    /// when the host trusts our CA (so deny paths are enforced by that user's permissions).
    pub fn read_mode(facts: &HostFacts) -> ExecMode {
        match facts.enforcement.jail {
            JailKind::None => ExecMode::Plain,
            k if facts.enforcement.certificates => ExecMode::JailedAsAgent(k),
            k => ExecMode::Jailed(k),
        }
    }

    /// Mode for writes: a forced-command certificate when the host trusts our CA.
    pub fn write_mode(facts: &HostFacts) -> ExecMode {
        if facts.enforcement.certificates {
            ExecMode::Certified
        } else {
            ExecMode::Plain
        }
    }

    // ------------------------------------------------------------ calls

    pub fn new_call(
        &self,
        tool: ToolName,
        host: Option<&str>,
        command: Option<String>,
        path: Option<String>,
        args: Value,
    ) -> ToolCall {
        ToolCall {
            run_id: self.run_id.clone(),
            call_id: uuid::Uuid::new_v4().to_string(),
            tool,
            host: host.map(str::to_string),
            command,
            path,
            args,
        }
    }

    /// Deny paths are enforced here as well as by the policy engine.
    pub fn path_denied(&self, path: &str) -> Option<String> {
        self.policy
            .deny_paths
            .iter()
            .find(|p| path_matches(p, path))
            .map(|p| format!("path matches deny rule '{p}'"))
    }

    /// Validate and normalise an agent-supplied path.
    pub fn clean_path(&self, path: &str) -> Result<String, ToolError> {
        normalize_path(path).map_err(|e| ToolError::Invalid(format!("{path}: {e}")))
    }

    /// Audit a call we refused ourselves (before or instead of the policy engine).
    pub fn deny_call(&self, call: &ToolCall, reason: &str) -> ToolError {
        let ev = AiEvent::Call {
            call: call.clone(),
            decision: Decision::Deny {
                reason: reason.to_string(),
            },
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Call: {e:#}");
        }
        ToolError::Denied(reason.to_string())
    }

    /// Ask the policy engine and audit the call. Fails closed when the audit write fails.
    pub fn decide_only(
        &self,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        facts: &HostFacts,
    ) -> Result<Decision, ToolError> {
        let decision = self.deps.engine.decide(&self.policy, call, analysis, facts);
        self.emit(&AiEvent::Call {
            call: call.clone(),
            decision: decision.clone(),
        })
        .map_err(|e| ToolError::Failed(format!("audit unavailable, refusing: {e:#}")))?;
        Ok(decision)
    }

    /// Block until a human decides. Returns who approved.
    pub async fn request_approval(
        &self,
        tier: Tier,
        subject: ApprovalSubject,
        summary: &str,
    ) -> Result<String, ToolError> {
        self.request_approval_because(tier, subject, summary, "")
            .await
    }

    /// `request_approval`, with the policy's reason shown to the approver.
    ///
    /// One deadline covers both routes. When the client can elicit and the tier allows it,
    /// the human is asked in-client (briefly, in asynchronous mode); otherwise the request
    /// goes out of band. In asynchronous mode the tool call returns `pending` while this
    /// keeps waiting, and the approved action still runs exactly once.
    pub async fn request_approval_because(
        &self,
        tier: Tier,
        subject: ApprovalSubject,
        summary: &str,
        reason: &str,
    ) -> Result<String, ToolError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let timeout = Duration::from_secs(self.config.ai.approval_timeout_secs.max(1));
        let deadline = Instant::now() + timeout;
        let mut req = ApprovalRequest {
            request_id: request_id.clone(),
            run_id: self.run_id.clone(),
            client: self.client(),
            tier,
            subject,
            created_at: now_rfc3339(),
            expires_at: rfc3339_in(timeout),
            reason: reason.to_string(),
        };
        if let Ok(mut p) = self.pending.lock() {
            p.insert(
                request_id.clone(),
                PendingInfo {
                    tier,
                    summary: summary.to_string(),
                    since: req.created_at.clone(),
                },
            );
        }
        let outcome = self.obtain_approval(&mut req, deadline).await;
        if let Ok(mut p) = self.pending.lock() {
            p.remove(&request_id);
        }
        let outcome = outcome.map_err(|e| ToolError::Failed(format!("approval: {e:#}")))?;
        if let Err(e) = self.emit(&AiEvent::Approval {
            request_id,
            run_id: self.run_id.clone(),
            outcome: outcome.clone(),
        }) {
            warn!("audit Approval: {e:#}");
        }
        match outcome {
            ApprovalOutcome::Approved { by, .. } => Ok(by),
            // A person said no: never counted by the circuit breaker.
            ApprovalOutcome::Denied { by, reason } => Err(ToolError::Denied(format!(
                "denied by {by}{}",
                reason.map(|r| format!(": {r}")).unwrap_or_default()
            ))),
            // Expiry says nothing about the agent's behaviour: not counted either.
            ApprovalOutcome::Expired => {
                let secs = timeout.as_secs();
                Err(ToolError::Denied(format!(
                    "approval request expired: nobody approved within {secs} s"
                )))
            }
        }
    }

    /// In-client attempt (when the client can elicit), then the broker.
    async fn obtain_approval(
        &self,
        req: &mut ApprovalRequest,
        deadline: Instant,
    ) -> anyhow::Result<ApprovalOutcome> {
        if req.tier == Tier::InClient {
            if let Some(e) = &self.deps.elicitor {
                if e.can_elicit() {
                    let left = deadline.saturating_duration_since(Instant::now());
                    // A person who is not answering in their client should not hold the
                    // tool call open for the whole window: hand it to the usual approvers.
                    let attempt = if self.async_approvals() {
                        left.min(IN_CLIENT_ATTEMPT)
                    } else {
                        left
                    };
                    match tokio::time::timeout(attempt, e.confirm(req)).await {
                        Ok(Ok(Some(true))) => {
                            return Ok(ApprovalOutcome::Approved {
                                by: "user".into(),
                                channel: "confirmed-in-client".into(),
                                grant: None,
                            })
                        }
                        Ok(Ok(Some(false))) => {
                            return Ok(ApprovalOutcome::Denied {
                                by: "user".into(),
                                reason: None,
                            })
                        }
                        Ok(Ok(None)) => debug!("client can't elicit; asking out of band"),
                        Ok(Err(e)) => {
                            warn!("elicitation failed ({e}); asking out of band")
                        }
                        Err(_) => debug!("client did not answer; asking out of band"),
                    }
                }
            }
            req.tier = Tier::OutOfBand;
        }
        let left = deadline.saturating_duration_since(Instant::now()).as_secs();
        self.mark_pending(&req.request_id, left);
        self.wait_for_approval(req.clone(), deadline).await
    }

    /// Wait for the broker, sending MCP progress notifications when the client asked for them
    /// and can be reached (stdio server with an elicitation peer).
    async fn wait_for_approval(
        &self,
        req: ApprovalRequest,
        deadline: Instant,
    ) -> anyhow::Result<ApprovalOutcome> {
        let progress = self
            .deps
            .elicitor
            .clone()
            .zip(super::server::current_progress_token());
        let request = self.deps.broker.request(req);
        let Some((elicitor, token)) = progress else {
            return request.await;
        };
        let mut request = std::pin::pin!(request);
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        // The first tick is immediate; the first message should not be.
        tick.tick().await;
        let mut ticks = 0.0f64;
        loop {
            tokio::select! {
                outcome = &mut request => return outcome,
                _ = tick.tick() => {
                    ticks += 1.0;
                    let left = deadline.saturating_duration_since(Instant::now()).as_secs();
                    let msg = format!(
                        "waiting for approval in the bssh desktop app or `bssh ai watch` \
                         ({left} s left)"
                    );
                    if let Err(e) = elicitor.progress(token.clone(), ticks, msg).await {
                        debug!("progress notification failed: {e}");
                    }
                }
            }
        }
    }

    /// decide -> audit -> approval when Ask. Ok means the call may proceed.
    pub async fn authorize(
        &self,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        facts: &HostFacts,
    ) -> Result<(), ToolError> {
        self.authorize_or_ask(call, analysis, facts, None).await
    }

    /// Like [`authorize`](Self::authorize), but `force_ask` makes a policy `Allow` ask a human
    /// anyway. Used where execution without enforcement is never silent: an unjailed,
    /// not-provably-read-only command.
    pub async fn authorize_or_ask(
        &self,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        facts: &HostFacts,
        force_ask: Option<&str>,
    ) -> Result<(), ToolError> {
        let (tier, reason) = match self.decide_only(call, analysis, facts)? {
            Decision::Allow { .. } if force_ask.is_none() => return Ok(()),
            Decision::Allow { .. } => (
                policy::tier_for_call(&self.policy, call, analysis, facts),
                force_ask.unwrap_or_default().to_string(),
            ),
            Decision::Deny { reason } => {
                self.note_denial(&reason);
                return Err(ToolError::Denied(reason));
            }
            Decision::Ask { tier, reason } => (tier, reason),
        };
        let subject = ApprovalSubject::Call {
            call: call.clone(),
            analysis: analysis.cloned(),
        };
        let summary = format!(
            "{} {}",
            super::tools::name_str(call.tool),
            call.command.as_deref().unwrap_or("")
        );
        debug!("approval needed ({tier:?}): {reason}");
        self.request_approval_because(tier, subject, summary.trim(), &reason)
            .await?;
        Ok(())
    }

    /// Execute, redact, audit `Result`, record output.
    pub async fn exec(
        &self,
        conn: &Connection,
        call: &ToolCall,
        command: &str,
        mode: ExecMode,
        limits: ExecLimits,
    ) -> Result<Executed, ToolError> {
        let jailed = mode.is_jailed();
        let raw = self
            .deps
            .executor
            .exec(conn, command, mode, limits)
            .await
            .map_err(|e| ToolError::Failed(format!("{}: {e:#}", conn.name)))?;
        let (stdout, r1) = self.deps.redactor.redact(&raw.stdout);
        let (stderr, r2) = self.deps.redactor.redact(&raw.stderr);
        let redactions = merge_redactions([r1, r2]);
        let result = ExecResult {
            stdout,
            stderr,
            ..raw
        };
        let digest =
            sha256_hex(format!("{}\n--stderr--\n{}", result.stdout, result.stderr).as_bytes());
        self.emit_result(
            call,
            result.exit_code,
            digest,
            redactions.clone(),
            Some((result.duration_ms, jailed)),
        );
        if let Err(e) =
            report::record_output(&self.config, &self.run_id, &call.call_id, command, &result)
        {
            warn!("record output: {e:#}");
        }
        Ok(Executed { result, redactions })
    }

    pub fn emit_result(
        &self,
        call: &ToolCall,
        exit_code: Option<i32>,
        output_sha256: String,
        redactions: Vec<Redaction>,
        timing: Option<(u64, bool)>,
    ) {
        let ev = AiEvent::Result {
            run_id: self.run_id.clone(),
            call_id: call.call_id.clone(),
            exit_code,
            output_sha256,
            redactions,
            duration_ms: timing.map(|t| t.0),
            jailed: timing.map(|t| t.1),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Result: {e:#}");
        }
    }

    /// Redact free text (file contents, log lines) and audit its `Result`.
    pub fn redact_text(&self, call: &ToolCall, text: &str) -> (String, Vec<Redaction>) {
        let (out, reds) = self.deps.redactor.redact(text);
        self.emit_result(call, None, sha256_hex(out.as_bytes()), reds.clone(), None);
        (out, reds)
    }

    /// Redact every string leaf of parsed remote data.
    pub fn redact_value(&self, v: &mut Value) -> Vec<Redaction> {
        let mut all = Vec::new();
        redact_leaves(self.deps.redactor.as_ref(), v, &mut all);
        merge_redactions([all])
    }

    // ------------------------------------------------------------ status

    pub fn status_snapshot(&self) -> Value {
        let now = Instant::now();
        let calls_used = self
            .calls
            .lock()
            .map(|c| {
                c.iter()
                    .filter(|t| now.duration_since(**t) <= Duration::from_secs(60))
                    .count()
            })
            .unwrap_or(0);
        let pending: Vec<Value> = self
            .pending
            .lock()
            .map(|p| {
                p.iter()
                    .map(|(id, i)| {
                        serde_json::json!({
                            "request_id": id, "tier": i.tier, "summary": i.summary, "since": i.since,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let grants: Vec<Value> = self
            .access
            .lock()
            .map(|g| {
                g.iter()
                    .filter(|g| g.expires > now)
                    .map(|g| {
                        serde_json::json!({
                            "scope": g.scope,
                            "expires_in_secs": g.expires.duration_since(now).as_secs(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let b = &self.policy.budgets;
        serde_json::json!({
            "run_id": self.run_id,
            "client": self.client(),
            "policy": self.policy_name,
            "profile": self.policy.profile,
            "scope": self.policy.scope,
            "budgets": {
                "calls_per_min": b.calls_per_min,
                "calls_left_this_minute": (b.calls_per_min as usize).saturating_sub(calls_used),
                "changes_per_run": b.changes_per_run,
                "changes_left": b.changes_per_run.saturating_sub(self.changes_used.load(Ordering::SeqCst)),
                "run_ttl": b.run_ttl,
                "ttl_left_secs": self.deadline.saturating_duration_since(now).as_secs(),
                "elapsed_secs": now.duration_since(self.started).as_secs(),
            },
            "pending_approvals": pending,
            "access_grants": grants,
            "stopped": self.is_stopped(),
            "paused": self.is_paused(),
            "pause_reason": self.pause_reason.lock().ok().and_then(|r| r.clone()),
        })
    }
}

fn poisoned() -> ToolError {
    ToolError::Failed("internal lock poisoned".into())
}

/// How long an in-client (T1) prompt may stay open before the request goes out of band.
const IN_CLIENT_ATTEMPT: Duration = Duration::from_secs(15);

/// Does this policy-engine denial say something about the agent's behaviour?
///
/// Human decisions and approvals never reach here; this also filters the refusals that are
/// not the agent probing or attacking: the observe profile, budget limits and malformed
/// calls (bad arguments are reported as `Invalid`, not through the engine).
fn counts_for_breaker(reason: &str) -> bool {
    !(reason.starts_with("profile.")
        || reason.starts_with("budget.")
        || reason.starts_with("target:")
        || reason.starts_with("denied by")
        || reason.contains("approval request expired"))
}

fn redact_leaves(r: &dyn Redactor, v: &mut Value, all: &mut Vec<Redaction>) {
    match v {
        Value::String(s) => {
            let (out, reds) = r.redact(s);
            if !reds.is_empty() {
                *s = out;
                all.extend(reds);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| redact_leaves(r, x, all)),
        Value::Object(o) => o.values_mut().for_each(|x| redact_leaves(r, x, all)),
        _ => {}
    }
}

/// Sum counts per kind.
pub fn merge_redactions<I: IntoIterator<Item = Vec<Redaction>>>(lists: I) -> Vec<Redaction> {
    let mut map: Vec<Redaction> = Vec::new();
    for r in lists.into_iter().flatten() {
        match map.iter_mut().find(|m| m.kind == r.kind) {
            Some(m) => m.count += r.count,
            None => map.push(r),
        }
    }
    map
}

/// Convenience for handlers: an `anyhow` error to a `ToolError`.
pub fn failed(msg: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(msg.to_string())
}

fn _assert_send_sync() {
    fn is<T: Send + Sync>() {}
    is::<RunContext>();
    let _ = anyhow!("x");
}
