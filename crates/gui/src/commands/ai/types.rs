//! Serializable views and event payloads of the AI hub.

use std::collections::BTreeMap;

use bayesian_ssh::services::ai::types::{AiEvent, ApprovalRequest, Grant, Policy};
use serde::Serialize;

/// One event of a run with the order it arrived in and when.
#[derive(Debug, Clone, Serialize)]
pub struct StoredEvent {
    /// 1-based, per run. The frontend drops anything it has already seen.
    pub seq: u64,
    /// RFC 3339, when the desktop received it.
    pub at: String,
    pub event: AiEvent,
}

/// What the AI view shows for one run.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    /// MCP client name and version, self-reported by the agent.
    pub client: String,
    pub policy: String,
    pub started_at: String,
    pub on_behalf_of: Option<String>,
    /// Tool calls seen since the desktop connected.
    pub calls: u32,
    pub denied: u32,
    pub pending: Vec<ApprovalRequest>,
    pub grants: Vec<Grant>,
    /// The run's socket is open: Stop and Decide work.
    pub connected: bool,
    /// Set by the run's `RunEnded` event.
    pub ended: Option<String>,
    /// The circuit breaker paused the run; the user can resume it.
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
}

/// `ai_settings`: the read-only part of `config.ai`.
#[derive(Debug, Serialize)]
pub struct AiSettingsView {
    pub policies: BTreeMap<String, Policy>,
    pub record_output: bool,
    pub approval_timeout_secs: u64,
    pub trusted_tag_sources: Vec<String>,
    /// The `config.json` these policies were read from, so the page can name it.
    pub config_path: String,
}
