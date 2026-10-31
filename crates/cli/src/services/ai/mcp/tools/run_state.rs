//! Tools about the run itself: `status`, `request_access` and `wait_approval`.

use super::convert::to_value;
use super::dispatch::WAIT_APPROVAL_MAX;
use super::inputs::{NoArgs, RequestAccessArgs, WaitApprovalArgs};
use super::outputs::AccessOut;
use crate::services::ai::mcp::context::{RunContext, ToolError, ToolResult};
use crate::services::ai::mcp::util::parse_duration;
use crate::services::ai::types::{
    AiEvent, ApprovalSubject, Decision, HostFacts, Scope, Tier, ToolName,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

const ACCESS_MAX_SECS: u64 = 8 * 3600;

pub(super) async fn status(ctx: &Arc<RunContext>, _a: NoArgs) -> ToolResult {
    let call = ctx.new_call(ToolName::Status, None, None, None, json!({}));
    // Diagnostics, never an action: status works while paused, stopped, over budget or past
    // the TTL, so the agent can always explain what happened.
    if let Err(e) = ctx.emit(&AiEvent::Call {
        call: call.clone(),
        decision: Decision::Allow {
            reason: "status is always allowed".into(),
        },
    }) {
        warn!("audit status: {e:#}");
    }
    let mut snap = ctx.status_snapshot();
    let changes: Vec<Value> = ctx
        .plans
        .list()
        .into_iter()
        .map(|(id, state, title)| json!({ "change_id": id, "state": state, "title": title }))
        .collect();
    if let Some(o) = snap.as_object_mut() {
        o.insert("changes".into(), Value::Array(changes));
    }
    Ok(snap)
}

pub(super) async fn request_access(ctx: &Arc<RunContext>, a: RequestAccessArgs) -> ToolResult {
    let dur =
        parse_duration(&a.duration).map_err(|e| ToolError::Invalid(format!("duration: {e}")))?;
    if dur.is_zero() || dur > Duration::from_secs(ACCESS_MAX_SECS) {
        return Err(ToolError::Invalid(
            "duration must be between 1s and 8h".into(),
        ));
    }
    if a.scope.tags.is_empty() && a.scope.hosts.is_empty() {
        return Err(ToolError::Invalid(
            "scope needs at least one tag or host".into(),
        ));
    }
    let scope = Scope {
        tags: a.scope.tags.clone(),
        hosts: a.scope.hosts.clone(),
    };
    let call = ctx.new_call(
        ToolName::RequestAccess,
        None,
        None,
        None,
        json!({ "scope": scope, "reason": a.reason, "duration": a.duration }),
    );
    // Widening scope always needs a human, whatever the engine would allow.
    let tier = match ctx.decide_only(&call, None, &HostFacts::default())? {
        Decision::Deny { reason } => {
            ctx.note_denial(&reason);
            return Err(ToolError::Denied(reason));
        }
        Decision::Ask { tier, .. } => tier,
        Decision::Allow { .. } => Tier::OutOfBand,
    };
    let subject = ApprovalSubject::Access {
        scope: scope.clone(),
        reason: a.reason.clone(),
        duration: a.duration.clone(),
    };
    let mut parts = Vec::new();
    if !scope.tags.is_empty() {
        parts.push(format!("tags {}", scope.tags.join(", ")));
    }
    if !scope.hosts.is_empty() {
        parts.push(format!("hosts {}", scope.hosts.join(", ")));
    }
    let summary = format!("access to {} for {}", parts.join(" · "), a.duration);
    let by = ctx.request_approval(tier, subject, &summary).await?;
    ctx.grant_access(scope, dur);
    to_value(AccessOut {
        status: "granted".into(),
        approved_by: by,
        expires_in_secs: dur.as_secs(),
    })
}

/// Collect the result of a call that returned `{status: "pending"}`.
///
/// The original handler runs once, in the background; this only waits for it. Repeated or
/// concurrent calls observe the same stored result, so the action never runs twice.
pub(super) async fn wait_approval(ctx: &Arc<RunContext>, a: WaitApprovalArgs) -> ToolResult {
    let request_id = a.request_id;
    let slot = ctx.pending_slot(&request_id).ok_or_else(|| {
        ToolError::Invalid(format!(
            "unknown request_id '{request_id}'; use the id from a result with status 'pending'"
        ))
    })?;
    let until = match slot.pending() {
        Some((_, _, expires_at)) => (Instant::now() + WAIT_APPROVAL_MAX).min(expires_at),
        None => Instant::now() + WAIT_APPROVAL_MAX,
    };
    match slot.wait_done(until).await {
        Some(Ok(result)) => Ok(json!({
            "status": "done",
            "request_id": request_id,
            "tool": slot.tool(),
            "result": result,
        })),
        Some(Err(e)) => Err(e),
        None => {
            let secs = slot.pending().map(|(_, s, _)| s).unwrap_or(0);
            Ok(json!({
                "status": "pending",
                "request_id": request_id,
                "expires_in_secs": secs,
                "next": "call wait_approval again with this request_id",
            }))
        }
    }
}
