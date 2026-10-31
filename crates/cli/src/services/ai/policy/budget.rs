//! Run budgets: duration parsing, and the circuit breaker.

use crate::services::ai::store::{AiStore, CounterKind};
use crate::services::ai::types::Budgets;
use std::time::Duration;

/// Parse `"2h"`, `"30m"`, `"45s"`, `"1d"`, `"1h30m"` or plain seconds.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<u64>() {
        return Some(Duration::from_secs(n));
    }
    let mut total = 0u64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        total += n.checked_mul(match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => return None,
        })?;
    }
    if !num.is_empty() {
        return None;
    }
    Some(Duration::from_secs(total))
}

/// Out-of-scope probes within [`PROBE_WINDOW`] that trip the breaker.
pub const PROBE_LIMIT: u32 = 3;
pub const PROBE_WINDOW: Duration = Duration::from_secs(300);

/// Circuit breaker (design section 8): `Some(reason)` when the run should pause.
/// Trips on too many denials in a minute (`budgets.max_denials_per_min`, 0 = off) or on
/// repeated probing of out-of-scope hosts. The MCP run context calls this after each decision, emits
/// `AiEvent::Paused`; the run is paused, not stopped.
pub fn circuit_breaker(store: &AiStore, run_id: &str, budgets: &Budgets) -> Option<String> {
    let minute = Duration::from_secs(60);
    if budgets.max_denials_per_min > 0 {
        let n = store
            .count_within(run_id, CounterKind::Denial, minute)
            .unwrap_or(0);
        if n >= budgets.max_denials_per_min {
            return Some(format!(
                "{n} calls denied in the last minute (limit {})",
                budgets.max_denials_per_min
            ));
        }
    }
    let probes = store
        .count_within(run_id, CounterKind::Probe, PROBE_WINDOW)
        .unwrap_or(0);
    if probes >= PROBE_LIMIT {
        return Some(format!(
            "{probes} attempts on hosts outside the policy scope in the last 5 minutes"
        ));
    }
    None
}
