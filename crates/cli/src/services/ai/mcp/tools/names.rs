//! Wire names of the tools.

use crate::services::ai::types::ToolName;

/// snake_case wire name of a tool.
pub fn name_str(t: ToolName) -> &'static str {
    match t {
        ToolName::ListHosts => "list_hosts",
        ToolName::HostInfo => "host_info",
        ToolName::Observe => "observe",
        ToolName::ReadFile => "read_file",
        ToolName::SearchLogs => "search_logs",
        ToolName::RunReadonly => "run_readonly",
        ToolName::RunMany => "run_many",
        ToolName::ProposeChange => "propose_change",
        ToolName::ApplyChange => "apply_change",
        ToolName::EditFile => "edit_file",
        ToolName::Service => "service",
        ToolName::Run => "run",
        ToolName::Undo => "undo",
        ToolName::Status => "status",
        ToolName::RequestAccess => "request_access",
        ToolName::WaitApproval => "wait_approval",
    }
}

pub const ALL_TOOLS: [ToolName; 16] = [
    ToolName::ListHosts,
    ToolName::HostInfo,
    ToolName::Observe,
    ToolName::ReadFile,
    ToolName::SearchLogs,
    ToolName::RunReadonly,
    ToolName::RunMany,
    ToolName::ProposeChange,
    ToolName::ApplyChange,
    ToolName::EditFile,
    ToolName::Service,
    ToolName::Run,
    ToolName::Undo,
    ToolName::Status,
    ToolName::RequestAccess,
    ToolName::WaitApproval,
];

pub fn parse_tool(name: &str) -> Option<ToolName> {
    ALL_TOOLS.iter().copied().find(|t| name_str(*t) == name)
}

#[cfg(test)]
mod tests {
    use super::{ALL_TOOLS, name_str, parse_tool};
    #[test]
    fn parse_tool_roundtrip() {
        for t in ALL_TOOLS {
            assert_eq!(parse_tool(name_str(t)), Some(t));
        }
        assert_eq!(parse_tool("shell"), None);
    }
}
