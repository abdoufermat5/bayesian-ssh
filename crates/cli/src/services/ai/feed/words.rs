//! Words and column widths shared by the feed's lines.

use crate::services::ai::types::{ToolCall, ToolName};
use crate::services::ai::ui::{self};

/// Width of the kind column, host column (minimum) and the clock.
pub(super) const KIND_W: usize = 14;
pub(super) const HOST_W: usize = 8;
pub(super) const CLOCK_W: usize = 8;
/// Longest run of allowed reads shown before the rest fold.
pub(super) const SHOWN_READS: usize = 3;

/// The kind of a call in words.
pub fn kind_word(tool: ToolName) -> &'static str {
    match tool {
        ToolName::ListHosts => "list hosts",
        ToolName::HostInfo => "host info",
        ToolName::Status => "status",
        ToolName::Observe => "observe",
        ToolName::ReadFile | ToolName::SearchLogs | ToolName::RunReadonly | ToolName::RunMany => {
            "read"
        }
        ToolName::Run => "run",
        ToolName::Service => "service",
        ToolName::EditFile => "edit",
        ToolName::ProposeChange => "change plan",
        ToolName::ApplyChange => "apply",
        ToolName::Undo => "undo",
        ToolName::RequestAccess => "access",
        ToolName::WaitApproval => "approval",
    }
}

/// Reads: what the folder may hide when allowed.
pub(super) fn is_read(tool: ToolName) -> bool {
    matches!(
        tool,
        ToolName::Observe
            | ToolName::ReadFile
            | ToolName::SearchLogs
            | ToolName::RunReadonly
            | ToolName::RunMany
    )
}

/// One line of plain text: control characters (an agent can put escape codes in a command or
/// a title) and runs of white space become single spaces.
pub(super) fn oneline(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a call is about, as a person would say it.
pub(super) fn call_text(call: &ToolCall) -> String {
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    let text = if let Some(c) = &call.command {
        c.clone()
    } else if let Some(p) = &call.path {
        p.clone()
    } else if call.tool == ToolName::Service {
        match (arg("action"), arg("unit")) {
            (Some(a), Some(u)) => format!("{a} {u}"),
            (None, Some(u)) => u.to_string(),
            _ => String::new(),
        }
    } else if let Some(t) = arg("title") {
        t.to_string()
    } else if let Some(c) = arg("change_id") {
        format!("change {}", ui::short_id(c))
    } else {
        String::new()
    };
    oneline(&text)
}

pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
