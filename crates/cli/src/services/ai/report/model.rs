//! The report of one run, as serialized for `--json`.

use crate::services::ai::types::Redaction;
use crate::services::ai::ui::Style;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::render::render_report;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCall {
    pub seq: u64,
    pub ts: String,
    pub call_id: String,
    pub tool: String,
    pub host: Option<String>,
    pub command: Option<String>,
    pub path: Option<String>,
    /// `allow`, `ask` or `deny`.
    pub decision: String,
    pub reason: String,
    pub exit_code: Option<i32>,
    pub output_sha256: Option<String>,
    pub redactions: Vec<Redaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportApproval {
    pub seq: u64,
    pub ts: String,
    pub request_id: String,
    /// `approved`, `denied` or `expired`.
    pub outcome: String,
    pub by: Option<String>,
    pub channel: Option<String>,
    pub reason: Option<String>,
}

/// One step of a change: what the plan said and how it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportStep {
    /// `step`, `verify` or `undo`.
    pub phase: String,
    pub id: String,
    pub host: Option<String>,
    /// In words: the command, `edit /etc/x`, `restart nginx`.
    pub action: String,
    /// `None`: the step never ran.
    pub ok: Option<bool>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    /// Remote output, redacted and cut when it was recorded. Untrusted.
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportChange {
    pub seq: u64,
    pub ts: String,
    pub change_id: String,
    pub plan_hash: String,
    pub applied: bool,
    pub undo_point: Option<String>,
    // The fields below come from the change record on disk, not the audit log.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// Written by the agent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub approved_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub approved_channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub approved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub steps_ok: Option<(usize, usize)>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checks_ok: Option<(usize, usize)>,
    #[serde(default)]
    pub undone: bool,
    #[serde(default)]
    pub steps: Vec<ReportStep>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub client: Option<String>,
    pub on_behalf_of: Option<String>,
    pub policy: Option<String>,
    pub policy_hash: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub end_reason: Option<String>,
    pub hosts: Vec<String>,
    pub calls: Vec<ReportCall>,
    pub approvals: Vec<ReportApproval>,
    pub changes: Vec<ReportChange>,
    /// Calls the policy denied.
    pub denials: Vec<String>,
    /// Redactions summed over the run, by kind.
    pub redactions: Vec<Redaction>,
    pub circuit_breaker: Vec<String>,
    /// Recordings (`.cast` file names) found for the run.
    pub recordings: Vec<String>,
}

impl RunReport {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn render(&self, style: &Style, now: DateTime<Utc>) -> String {
        render_report(self, style, now)
    }

    pub fn to_text(&self) -> String {
        self.render(&Style::terminal(), Utc::now())
    }
}
