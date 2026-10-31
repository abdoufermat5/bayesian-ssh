//! Active grants and which tools need a host.

use super::glob::{glob_match, normalize_path, GlobMode};
use crate::services::ai::types::{Grant, ToolCall, ToolName};

pub(super) fn needs_host(tool: ToolName) -> bool {
    !matches!(
        tool,
        ToolName::ListHosts
            | ToolName::Status
            | ToolName::RequestAccess
            | ToolName::ProposeChange
            | ToolName::ApplyChange
            | ToolName::Undo
            // `run_many` names its hosts in the arguments; the aggregate facts carry their
            // tags. `wait_approval` never reaches the engine with a host.
            | ToolName::RunMany
            | ToolName::WaitApproval
    )
}

pub(super) fn grant_matches(g: &Grant, call: &ToolCall, host: &str) -> bool {
    if g.tool != call.tool || !glob_match(&g.hosts, host, GlobMode::Text) {
        return false;
    }
    if let Some(run) = &g.run_id
        && *run != call.run_id
    {
        return false;
    }
    match &g.target {
        None => true,
        Some(pat) => {
            let subject = match call.tool {
                ToolName::EditFile | ToolName::ReadFile => call.path.as_deref().map(normalize_path),
                ToolName::Service => call
                    .args
                    .get("unit")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                _ => call.command.clone(),
            };
            let mode = if call.tool == ToolName::EditFile {
                GlobMode::Path
            } else {
                GlobMode::Text
            };
            subject.is_some_and(|s| glob_match(pat, &s, mode))
        }
    }
}
