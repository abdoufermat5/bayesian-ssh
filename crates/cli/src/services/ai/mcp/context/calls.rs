//! Building, denying and deciding tool calls.

use super::error::ToolError;
use super::run_context::RunContext;
use crate::services::ai::mcp::util::{normalize_path, path_matches};
use crate::services::ai::types::{AiEvent, Analysis, Decision, HostFacts, ToolCall, ToolName};
use anyhow::Result;
use serde_json::Value;
use tracing::warn;

impl RunContext {
    pub fn new_call(
        &self,
        tool: ToolName,
        host: Option<&str>,
        command: Option<String>,
        path: Option<String>,
        args: Value,
    ) -> ToolCall {
        ToolCall {
            run_id: self.run_id.clone(),
            call_id: uuid::Uuid::new_v4().to_string(),
            tool,
            host: host.map(str::to_string),
            command,
            path,
            args,
        }
    }

    /// Deny paths are enforced here as well as by the policy engine.
    pub fn path_denied(&self, path: &str) -> Option<String> {
        self.policy
            .deny_paths
            .iter()
            .find(|p| path_matches(p, path))
            .map(|p| format!("path matches deny rule '{p}'"))
    }

    /// Validate and normalise an agent-supplied path.
    pub fn clean_path(&self, path: &str) -> Result<String, ToolError> {
        normalize_path(path).map_err(|e| ToolError::Invalid(format!("{path}: {e}")))
    }

    /// Audit a call we refused ourselves (before or instead of the policy engine).
    pub fn deny_call(&self, call: &ToolCall, reason: &str) -> ToolError {
        let ev = AiEvent::Call {
            call: call.clone(),
            decision: Decision::Deny {
                reason: reason.to_string(),
            },
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Call: {e:#}");
        }
        ToolError::Denied(reason.to_string())
    }

    /// Ask the policy engine and audit the call. Fails closed when the audit write fails.
    pub fn decide_only(
        &self,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        facts: &HostFacts,
    ) -> Result<Decision, ToolError> {
        let decision = self.deps.engine.decide(&self.policy, call, analysis, facts);
        self.emit(&AiEvent::Call {
            call: call.clone(),
            decision: decision.clone(),
        })
        .map_err(|e| ToolError::Failed(format!("audit unavailable, refusing: {e:#}")))?;
        Ok(decision)
    }
}
