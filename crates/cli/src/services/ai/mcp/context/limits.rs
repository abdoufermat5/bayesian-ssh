//! Per-run budgets: call rate, change count and the denial circuit breaker.

use super::error::ToolError;
use super::run_context::RunContext;
use anyhow::Result;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::debug;

impl RunContext {
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
}

fn poisoned() -> ToolError {
    ToolError::Failed("internal lock poisoned".into())
}

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
