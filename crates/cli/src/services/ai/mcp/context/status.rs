//! The `status` snapshot.

use super::run_context::RunContext;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl RunContext {
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
