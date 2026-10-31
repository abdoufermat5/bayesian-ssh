//! Approval requests: in-client elicitation first, then the broker.

use super::call_slot::{current_call_slot, CallSlot};
use super::error::ToolError;
use super::run_context::PendingInfo;
use super::run_context::RunContext;
use crate::services::ai::approval::Elicitor;
use crate::services::ai::mcp::util::{now_rfc3339, rfc3339_in};
use crate::services::ai::policy;
use crate::services::ai::types::{
    AiEvent, Analysis, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, HostFacts,
    Tier, ToolCall,
};
use anyhow::Result;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

impl RunContext {
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

    /// Block until a human decides. Returns who approved.
    pub async fn request_approval(
        &self,
        tier: Tier,
        subject: ApprovalSubject,
        summary: &str,
    ) -> Result<String, ToolError> {
        self.request_approval_because(ApprovalPrompt {
            tier,
            subject,
            summary,
            reason: "",
        })
        .await
    }

    /// `request_approval`, with the policy's reason shown to the approver.
    ///
    /// One deadline covers both routes. When the client can elicit and the tier allows it,
    /// the human is asked in-client (briefly, in asynchronous mode); otherwise the request
    /// goes out of band. In asynchronous mode the tool call returns `pending` while this
    /// keeps waiting, and the approved action still runs exactly once.
    async fn request_approval_because(
        &self,
        prompt: ApprovalPrompt<'_>,
    ) -> Result<String, ToolError> {
        let ApprovalPrompt {
            tier,
            subject,
            summary,
            reason,
        } = prompt;
        let request_id = uuid::Uuid::new_v4().to_string();
        let timeout = Duration::from_secs(self.config.ai.approval_timeout_secs.max(1));
        let deadline = Instant::now() + timeout;
        let req = ApprovalRequest {
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
        let outcome = self.obtain_approval(req, deadline).await;
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
        mut req: ApprovalRequest,
        deadline: Instant,
    ) -> anyhow::Result<ApprovalOutcome> {
        if req.tier == Tier::InClient {
            if let Some(e) = &self.deps.elicitor
                && e.can_elicit()
            {
                let left = deadline.saturating_duration_since(Instant::now());
                // A person who is not answering in their client should not hold the
                // tool call open for the whole window: hand it to the usual approvers.
                let attempt = if self.async_approvals() {
                    left.min(IN_CLIENT_ATTEMPT)
                } else {
                    left
                };
                match tokio::time::timeout(attempt, e.confirm(&req)).await {
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
            req.tier = Tier::OutOfBand;
        }
        let left = deadline.saturating_duration_since(Instant::now()).as_secs();
        self.mark_pending(&req.request_id, left);
        self.wait_for_approval(req, deadline).await
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
            .zip(crate::services::ai::mcp::server::current_progress_token());
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
            crate::services::ai::mcp::tools::name_str(call.tool),
            call.command.as_deref().unwrap_or("")
        );
        debug!("approval needed ({tier:?}): {reason}");
        self.request_approval_because(ApprovalPrompt {
            tier,
            subject,
            summary: summary.trim(),
            reason: &reason,
        })
        .await?;
        Ok(())
    }
}

/// What an approver is asked: the request's tier and subject, a one-line summary for `status`,
/// and the policy's reason.
struct ApprovalPrompt<'a> {
    tier: Tier,
    subject: ApprovalSubject,
    summary: &'a str,
    reason: &'a str,
}

/// How long an in-client (T1) prompt may stay open before the request goes out of band.
const IN_CLIENT_ATTEMPT: Duration = Duration::from_secs(15);
