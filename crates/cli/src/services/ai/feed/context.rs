//! What later feed lines need to remember about earlier events.

use crate::services::ai::types::{ApprovalRequest, ApprovalSubject, ToolName};
use crate::services::ai::ui::{self};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};

use super::line::FeedLine;
use super::words::oneline;

#[derive(Debug, Clone)]
pub(super) struct CallInfo {
    pub(super) tool: ToolName,
    pub(super) host: Option<String>,
    pub(super) text: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RunStats {
    pub(super) client: String,
    pub(super) calls: usize,
    pub(super) denied: usize,
    pub(super) changes: usize,
}

/// What later lines need to know about earlier events.
#[derive(Debug, Clone)]
pub struct FeedContext {
    /// Columns a line may use.
    pub width: usize,
    pub(super) calls: HashMap<String, CallInfo>,
    /// request id -> "plan 486c2f5a" / "request 440947ca"
    pub(super) requests: HashMap<String, String>,
    /// plan hash -> hosts of its steps
    pub(super) plan_hosts: HashMap<String, String>,
    pub(super) runs: HashMap<String, RunStats>,
    /// Allowed reads waiting for their result, which brings duration and jail.
    pub(super) held: Vec<(String, FeedLine, DateTime<Utc>)>,
    /// The line just returned was an allowed read (for the folder).
    pub(super) last_read: bool,
    /// Give out-of-band asks a line of their own. `watch` shows the approval card instead;
    /// a log has no cards.
    pub show_asks: bool,
}

impl Default for FeedContext {
    fn default() -> Self {
        Self::with_width(ui::width())
    }
}

impl FeedContext {
    /// Whether the line just returned by `event_feed` was an allowed read.
    pub fn last_was_read(&self) -> bool {
        self.last_read
    }

    /// Reads still waiting for their result after `max_age` (a failed call never sends
    /// one), released without timing so no call goes unshown.
    pub fn take_held(&mut self, now: DateTime<Utc>, max_age: chrono::Duration) -> Vec<FeedLine> {
        let (old, keep) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|(_, _, at)| now - *at >= max_age);
        self.held = keep;
        old.into_iter().map(|(_, l, _)| l).collect()
    }
}

impl FeedContext {
    pub fn with_width(width: usize) -> Self {
        Self {
            width,
            calls: HashMap::new(),
            requests: HashMap::new(),
            plan_hosts: HashMap::new(),
            runs: HashMap::new(),
            held: Vec::new(),
            last_read: false,
            show_asks: false,
        }
    }

    /// Remember a run's client name (from the control socket's hello or `RunStarted`).
    pub fn note_run(&mut self, run_id: &str, client: &str) {
        self.runs.entry(run_id.to_string()).or_default().client = oneline(client);
    }

    /// Remember an approval request so its outcome can name the plan.
    pub fn note_request(&mut self, req: &ApprovalRequest) {
        let label = match &req.subject {
            ApprovalSubject::Plan {
                plan_hash, plan, ..
            } => {
                self.plan_hosts
                    .insert(plan_hash.clone(), plan_hosts(plan).join(", "));
                format!("plan {}", ui::short_id(plan_hash))
            }
            _ => format!("request {}", ui::short_id(&req.request_id)),
        };
        self.requests.insert(req.request_id.clone(), label);
    }

    /// Host a call was made on.
    pub fn call_host(&self, call_id: &str) -> Option<&str> {
        self.calls.get(call_id)?.host.as_deref()
    }

    pub(super) fn client(&self, run_id: &str) -> String {
        self.runs
            .get(run_id)
            .map(|r| r.client.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "agent".into())
    }
}

pub(super) fn plan_hosts(plan: &crate::services::ai::types::ChangePlan) -> Vec<String> {
    let set: BTreeSet<String> = plan
        .steps
        .iter()
        .chain(&plan.verify)
        .filter_map(|s| s.host.as_deref().map(oneline))
        .collect();
    set.into_iter().collect()
}
