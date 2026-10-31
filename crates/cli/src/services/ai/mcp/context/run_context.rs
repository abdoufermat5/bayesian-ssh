//! `RunContext` and its construction: injected dependencies and per-run state.

use super::call_slot::CallSlot;
use super::inventory::{DbHosts, HostSource, ScopeFn};
use crate::config::AppConfig;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::approval::{serve_control, Elicitor, LocalBroker, RunInfo};
use crate::services::ai::audit::LocalAudit;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::mcp::files::{FileAccess, SftpFiles};
use crate::services::ai::mcp::server::McpElicitor;
use crate::services::ai::mcp::util::{now_rfc3339, parse_duration};
use crate::services::ai::plan::PlanStore;
use crate::services::ai::policy::DefaultPolicyEngine;
use crate::services::ai::redact::SecretRedactor;
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    AiEvent, Analyzer, ApprovalBroker, AuditSink, Enforcement, ExecResult, Executor, Policy,
    PolicyEngine, Redaction, Redactor, Scope, Tier,
};
use crate::services::ai::{ai_dir, policy};
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tracing::warn;

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

pub(super) struct AccessGrant {
    pub(super) scope: Scope,
    pub(super) expires: Instant,
}

/// A request waiting for a human, for `status`.
#[derive(Debug, Clone)]
pub(super) struct PendingInfo {
    pub(super) tier: Tier,
    pub(super) summary: String,
    pub(super) since: String,
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
    pub(super) events: broadcast::Sender<AiEvent>,
    pub(super) stop_tx: watch::Sender<bool>,
    pub(super) stopped: AtomicBool,
    /// Paused by the circuit breaker. Different from `stopped`: a person can resume it.
    pub(super) paused: AtomicBool,
    /// Why it paused, for `status` and the agent-facing error.
    pub(super) pause_reason: Mutex<Option<String>>,
    /// Asynchronous approvals: a call that asks returns `pending` instead of blocking,
    /// and `wait_approval` collects the result. On for real MCP transports; off in tests
    /// that drive the broker synchronously.
    pub(super) async_approvals: AtomicBool,
    /// Results of calls waiting for approval, by request id, kept until the run ends.
    pub(super) approval_slots: Mutex<HashMap<String, Arc<CallSlot>>>,
    pub(super) started: Instant,
    pub(super) deadline: Instant,
    pub(super) client: Mutex<String>,
    pub(super) start_emitted: AtomicBool,
    /// The local broker behind the control socket, told the client name on `start`.
    pub(super) local_broker: std::sync::OnceLock<Arc<LocalBroker>>,
    pub(super) end_emitted: AtomicBool,
    pub(super) calls: Mutex<VecDeque<Instant>>,
    pub(super) denials: Mutex<VecDeque<Instant>>,
    pub(super) facts: tokio::sync::Mutex<HashMap<String, Enforcement>>,
    /// Inventory source names by source id, resolved once for tag trust.
    pub(super) source_names: Mutex<HashMap<uuid::Uuid, Option<String>>>,
    pub(super) access: Mutex<Vec<AccessGrant>>,
    pub(super) pending: Mutex<HashMap<String, PendingInfo>>,
    pub(super) changes_used: AtomicU32,
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
            Arc::new(move |by: &str| resume_ctx.resume(by).map_err(|e| e.to_string()));
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
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RunContext>();
};
