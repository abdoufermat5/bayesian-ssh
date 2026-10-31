//! Rendering of a tool outcome into MCP `CallToolResult` content.

use super::change::{
    render_access, render_apply, render_edit_file, render_pending, render_propose, render_undo,
    render_wait_approval,
};
use super::error::render_tool_error;
use super::exec::{
    render_exec_out, render_read_file, render_run_many, render_search_logs, render_service,
};
use super::hosts::{render_host_info, render_list_hosts, render_status};
use super::observe::{format_clean_value, render_observe};
use crate::services::ai::mcp::context::ToolError;
use crate::services::ai::mcp::util::random_fence_tag;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::Value;

/// Render a successful tool JSON result into plain, human-readable text.
pub fn render_tool_result(tool: &str, v: &Value) -> String {
    // Any tool may answer a call that needs a human with the pending envelope.
    if v["status"].as_str() == Some("pending") && v.get("request_id").is_some() {
        return render_pending(v);
    }
    let tag = random_fence_tag();
    match tool {
        "run_readonly" | "run" => {
            let host = v["host"].as_str().unwrap_or("-");
            render_exec_out(host, v, &tag)
        }
        "service" => render_service(v, &tag),
        "run_many" => render_run_many(v, &tag),
        "read_file" => render_read_file(v, &tag),
        "search_logs" => render_search_logs(v, &tag),
        "observe" => render_observe(v, &tag),
        "list_hosts" => render_list_hosts(v),
        "host_info" => render_host_info(v, &tag),
        "status" => render_status(v),
        "propose_change" => render_propose(v),
        "apply_change" => render_apply(v, &tag),
        "edit_file" => render_edit_file(v),
        "undo" => render_undo(v, &tag),
        "request_access" => render_access(v),
        "wait_approval" => render_wait_approval(v),
        _ => {
            if v.get("untrusted_data").is_some() {
                render_observe(v, &tag)
            } else if v.get("bytes_returned").is_some() {
                render_read_file(v, &tag)
            } else if v.get("hosts").is_some() && v.get("count").is_some() {
                render_list_hosts(v)
            } else if v.get("results").is_some() && v.get("count").is_some() {
                render_run_many(v, &tag)
            } else if v.get("status").is_some() && v.get("failed_step").is_some() {
                render_undo(v, &tag)
            } else if v.get("change_id").is_some() && v.get("steps").is_some() {
                render_propose(v)
            } else if v.get("unit").is_some() && v.get("action").is_some() {
                render_service(v, &tag)
            } else if v.get("untrusted_output").is_some() {
                let host = v["host"].as_str().unwrap_or("-");
                render_exec_out(host, v, &tag)
            } else if v.get("budgets").is_some() {
                render_status(v)
            } else if v.get("expires_in_secs").is_some() {
                render_access(v)
            } else {
                format_clean_value(v)
            }
        }
    }
}

pub fn to_result(tool: &str, r: Result<serde_json::Value, ToolError>) -> CallToolResult {
    match r {
        Ok(v) => {
            let text = render_tool_result(tool, &v);
            let mut res = CallToolResult::structured(v);
            res.content = vec![ContentBlock::text(text)];
            res
        }
        Err(e) => {
            let text = render_tool_error(tool, &e);
            CallToolResult::error(vec![ContentBlock::text(text)])
        }
    }
}
