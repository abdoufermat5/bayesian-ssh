//! Shared types and traits for bssh AI.
//!
//! Every part of `services::ai` codes against these. Changing a public item
//! here is a contract change: say so in the PR description.

use crate::models::Connection;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;

// ---------------------------------------------------------------- analysis

/// Risk tags produced by the command analyzer (design 6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTag {
    Read,
    Write,
    ServiceChange,
    Destructive,
    Privilege,
    Network,
    ExfilRisk,
    Unknown,
}

/// One simple command found in the AST, after unwrapping `sudo`, `env`, `sh -c`...
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleCommand {
    /// Program as written (`systemctl`, `/usr/bin/cat`).
    pub program: String,
    pub args: Vec<String>,
    /// Wrappers peeled off, outermost first (`["sudo", "timeout"]`).
    pub wrappers: Vec<String>,
    /// Redirection targets written to (`> /etc/x`, `>> log`).
    pub writes_to: Vec<String>,
    pub tags: BTreeSet<RiskTag>,
    /// Rule that classified it, for `bssh ai explain`.
    pub rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    pub commands: Vec<SimpleCommand>,
    /// Union of every command's tags.
    pub tags: BTreeSet<RiskTag>,
    /// Set when the command can't be fully analyzed (parse error, eval, obfuscation...).
    /// Implies `RiskTag::Unknown` in `tags`.
    pub unknown_reason: Option<String>,
}

impl Analysis {
    pub fn is_read_only(&self) -> bool {
        self.unknown_reason.is_none() && self.tags.iter().all(|t| *t == RiskTag::Read)
    }
}

pub trait Analyzer: Send + Sync {
    fn analyze(&self, command: &str) -> Analysis;
}

// ---------------------------------------------------------------- redaction

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redaction {
    /// Kind shown in the replacement marker, e.g. `aws-key`, `private-key`, `vault-secret`.
    pub kind: String,
    pub count: usize,
}

pub trait Redactor: Send + Sync {
    /// Returns the redacted text (markers look like `«redacted:aws-key»`) and what was removed.
    fn redact(&self, text: &str) -> (String, Vec<Redaction>);
}

// ---------------------------------------------------------------- policy

/// Approval tiers (design 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// MCP elicitation through the agent's client (T1). Opt-in, non-prod only.
    InClient,
    /// Desktop app, `bssh ai approve`, Teams admin devices (T2).
    OutOfBand,
    /// Two T2 approvals (T3, Teams).
    TwoPerson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum Decision {
    Allow { reason: String },
    Ask { tier: Tier, reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    Observe,
    Operate,
    Oncall,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scope {
    /// Hosts carrying any of these tags are in scope. Empty = no host.
    pub tags: Vec<String>,
    /// Extra hosts by connection name.
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApprovalRules {
    /// Tier used for anything that asks, unless a tag rule below applies.
    pub default: Tier,
    /// Tags where T1 (in-client) is allowed for low-risk reversible changes.
    pub in_client: Vec<String>,
    /// Tags that need T3.
    pub two_person: Vec<String>,
}

impl Default for ApprovalRules {
    fn default() -> Self {
        Self {
            default: Tier::OutOfBand,
            in_client: Vec::new(),
            two_person: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Budgets {
    pub calls_per_min: u32,
    pub changes_per_run: u32,
    /// Humantime-style duration, e.g. `"2h"`.
    pub run_ttl: String,
    /// Denials within one minute that trip the circuit breaker.
    pub max_denials_per_min: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            calls_per_min: 30,
            changes_per_run: 10,
            run_ttl: "2h".into(),
            max_denials_per_min: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactRules {
    pub vault_secrets: bool,
    /// `"default"` or `"off"`.
    pub patterns: String,
}

impl Default for RedactRules {
    fn default() -> Self {
        Self {
            vault_secrets: true,
            patterns: "default".into(),
        }
    }
}

/// A named agent policy (design 6.4). Lives in `config.json` under `ai.policies`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub profile: Profile,
    pub scope: Scope,
    pub approval: ApprovalRules,
    pub read_paths: Vec<String>,
    pub deny_paths: Vec<String>,
    pub write_paths: Vec<String>,
    /// Matched on the AST, not the raw text.
    pub deny: Vec<String>,
    pub budgets: Budgets,
    pub redact: RedactRules,
}

/// What the policy engine knows about the target host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    pub name: String,
    pub tags: Vec<String>,
    pub enforcement: Enforcement,
}

/// How strongly a host is enforced (design 7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enforcement {
    pub jail: JailKind,
    /// Host trusts the bssh CA, so per-step forced-command certificates work.
    pub certificates: bool,
    /// The `bssh-agent` user exists.
    pub agent_user: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JailKind {
    Landlock,
    Bubblewrap,
    #[default]
    None,
}

// ---------------------------------------------------------------- tool calls

/// Tool names, stable on the wire (MCP) and in the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    ListHosts,
    HostInfo,
    Observe,
    ReadFile,
    SearchLogs,
    RunReadonly,
    RunMany,
    ProposeChange,
    ApplyChange,
    EditFile,
    Service,
    Run,
    Undo,
    Status,
    RequestAccess,
}

impl ToolName {
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Self::ProposeChange
                | Self::ApplyChange
                | Self::EditFile
                | Self::Service
                | Self::Run
                | Self::Undo
        )
    }
}

/// One call the agent made, as the policy engine and the audit see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub run_id: String,
    pub call_id: String,
    pub tool: ToolName,
    pub host: Option<String>,
    /// Shell command when the tool runs one (`run`, `run_readonly`, plan steps).
    pub command: Option<String>,
    /// File path for `read_file` / `edit_file`.
    pub path: Option<String>,
    /// Raw arguments as the agent sent them.
    pub args: serde_json::Value,
}

pub trait PolicyEngine: Send + Sync {
    fn decide(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision;
}

// ---------------------------------------------------------------- change plans

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    Run { command: String },
    EditFile { path: String, patch: String },
    Service { unit: String, action: ServiceAction },
    Undo { step: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Reload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// `step-1`, `step-2`... assigned by `propose_change`.
    pub id: String,
    pub host: Option<String>,
    pub action: StepAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlan {
    pub change_id: String,
    pub run_id: String,
    pub title: String,
    /// Written by the agent. Shown labelled "the agent says".
    pub reason: String,
    pub steps: Vec<Step>,
    pub verify: Vec<Step>,
    pub rollback: Vec<Step>,
}

impl ChangePlan {
    /// SHA-256 hex over the canonical JSON of title, steps, verify and rollback.
    /// Approvals sign this hash; `apply_change` refuses a plan whose hash changed.
    pub fn hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let canon = serde_json::json!({
            "title": self.title, "steps": self.steps, "verify": self.verify, "rollback": self.rollback,
        });
        Sha256::digest(canon.to_string().as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

// ---------------------------------------------------------------- approvals

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalSubject {
    /// A single call (e.g. `run` outside a plan).
    Call {
        call: ToolCall,
        analysis: Option<Analysis>,
    },
    /// A whole change plan; approval covers `plan_hash`.
    Plan {
        plan: ChangePlan,
        plan_hash: String,
        analyses: Vec<Option<Analysis>>,
    },
    /// `request_access`: wider scope for a while.
    Access {
        scope: Scope,
        reason: String,
        duration: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub run_id: String,
    /// MCP client name/version, self-reported.
    pub client: String,
    pub tier: Tier,
    pub subject: ApprovalSubject,
    /// RFC 3339.
    pub created_at: String,
    pub expires_at: String,
    /// Why the policy asks (`read: /opt/app/app.py is outside read_paths`), for single
    /// calls. Empty for plans and for requests written before it existed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ApprovalOutcome {
    Approved {
        by: String,
        channel: String,
        grant: Option<Grant>,
    },
    Denied {
        by: String,
        reason: Option<String>,
    },
    Expired,
}

/// Time-boxed permission that turns matching asks into allows (design 6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub grant_id: String,
    pub run_id: Option<String>,
    pub tool: ToolName,
    /// Glob over host names.
    pub hosts: String,
    /// Optional glob over paths (edit_file) or service units (service).
    pub target: Option<String>,
    /// RFC 3339.
    pub expires_at: String,
}

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ApprovalBroker: Send + Sync {
    /// Blocks until approved, denied or expired.
    async fn request(&self, req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome>;
}

// ---------------------------------------------------------------- execution

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecLimits {
    pub timeout: Duration,
    /// Per stream (stdout and stderr each).
    pub max_output_bytes: usize,
}

impl Default for ExecLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            max_output_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    /// None when killed by timeout.
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
}

/// How a command reaches the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecMode {
    /// As the connection's normal user and auth (delegation without host setup).
    Plain,
    /// Wrapped in the host's read-only jail.
    Jailed(JailKind),
    /// As `bssh-agent` with a short-lived certificate whose force-command is exactly this command.
    Certified,
}

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Executor: Send + Sync {
    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
        mode: ExecMode,
        limits: ExecLimits,
    ) -> anyhow::Result<ExecResult>;
}

// ---------------------------------------------------------------- audit

/// Everything that goes to the run's audit trail and the live view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AiEvent {
    RunStarted {
        run_id: String,
        client: String,
        on_behalf_of: String,
        policy: String,
        policy_hash: String,
    },
    Call {
        call: ToolCall,
        decision: Decision,
    },
    Approval {
        request_id: String,
        run_id: String,
        outcome: ApprovalOutcome,
    },
    Result {
        run_id: String,
        call_id: String,
        exit_code: Option<i32>,
        output_sha256: String,
        redactions: Vec<Redaction>,
        /// Commands only. Absent from events written before it existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// Whether the command ran in the host's read-only jail.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        jailed: Option<bool>,
    },
    Change {
        run_id: String,
        change_id: String,
        plan_hash: String,
        applied: bool,
        undo_point: Option<String>,
    },
    CircuitBreaker {
        run_id: String,
        reason: String,
    },
    RunEnded {
        run_id: String,
        reason: String,
    },
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AiEvent) -> anyhow::Result<()>;
}
