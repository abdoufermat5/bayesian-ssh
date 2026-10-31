//! Why a tool call produced no result, and the `Result` alias handlers return.

use anyhow::Result;
use serde_json::Value;

/// Why a tool call did not produce a result. Rendered to the agent as a tool error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// Policy, approval, budget or stop said no.
    Denied(String),
    /// Bad arguments.
    Invalid(String),
    /// Something broke (host unreachable, parse failure...).
    Failed(String),
    /// Something broke after the call had already run on the host (a parse failure, a
    /// partial change). The agent is told the command ran, not "Nothing ran.".
    FailedAfterRun(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Denied(m) => write!(f, "denied: {m}"),
            ToolError::Invalid(m) => write!(f, "invalid request: {m}"),
            ToolError::Failed(m) => write!(f, "failed: {m}"),
            ToolError::FailedAfterRun(m) => write!(f, "failed after running: {m}"),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError::Failed(format!("{e:#}"))
    }
}

/// Why [`RunContext::resume`](super::RunContext::resume) refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResumeError {
    /// The run was stopped, and stop is final.
    #[error("the run is stopped and cannot be resumed; start a new agent session")]
    Stopped,
    /// No circuit-breaker pause is in effect.
    #[error("the run is not paused")]
    NotPaused,
}

pub type ToolResult = Result<Value, ToolError>;

/// Convenience for handlers: an `anyhow` error to a `ToolError`.
pub fn failed(msg: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(msg.to_string())
}
