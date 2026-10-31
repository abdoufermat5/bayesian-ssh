//! Plain-words rendering of a `ToolError`, with the next step for the agent.

use crate::services::ai::mcp::context::ToolError;

/// Seconds from `approval request expired: nobody approved within 300 s`.
pub(super) fn approval_wait_seconds(msg: &str) -> Option<&str> {
    let rest = msg.split("within ").nth(1)?.trim();
    let secs = rest.split_whitespace().next()?;
    secs.chars().all(|c| c.is_ascii_digit()).then_some(secs)
}

/// Render a ToolError into plain words + 'Nothing ran.' (only when true) + next step.
pub fn render_tool_error(_tool: &str, err: &ToolError) -> String {
    match err {
        ToolError::Denied(msg) => {
            let clean = msg.trim_end_matches('.');
            let next: String = if clean.contains("profile.observe")
                || clean.contains("observe profile")
            {
                "This run is read-only (observe profile). Tell the user; changes need a policy \
                 with profile operate (see `bssh ai policy list`)."
                    .into()
            } else if clean.contains("no read-only jail") {
                "This read needs the host's read-only jail or approval, and the host has no jail \
                 to enforce it. Tell the user (`bssh ai doctor` shows the host); use observe or \
                 search_logs to read meanwhile."
                    .into()
            } else if clean.contains("unknown host")
                || clean.contains("out-of-scope")
                || clean.contains("not in this policy's scope")
            {
                "Use list_hosts to see the hosts you can reach.".into()
            } else if clean.contains("paused by the circuit breaker") {
                "The run is paused; it was not stopped. Tell the user, then wait: they can \
                 resume it with `bssh ai resume` or in the desktop app. Do not retry or restart \
                 the agent."
                    .into()
            } else if clean.contains("stopped") || clean.contains("time limit") {
                "The run is stopped; only the user can start a new agent session. Tell them, and \
                 do not retry."
                    .into()
            } else if clean.contains("expired") {
                match approval_wait_seconds(clean) {
                    Some(secs) => format!(
                        "Nobody approved within {secs} s. Ask the user to approve in the bssh \
                         desktop app or with `bssh ai watch`, then retry."
                    ),
                    None => "Nobody approved in time. Ask the user to approve in the bssh desktop \
                             app or with `bssh ai watch`, then retry."
                        .into(),
                }
            } else if clean.contains("denied by") || clean.contains("declined") {
                "The request was declined. Tell the user what you wanted to do and why; do not \
                 retry the same call."
                    .into()
            } else if clean.contains("budget") || clean.contains("calls per minute") {
                "Wait before retrying, or check status for remaining budgets.".into()
            } else if clean.contains("path") {
                "Use an allowed path, or use observe or search_logs instead.".into()
            } else {
                "Tell the user what you tried. To ask for wider host scope, use request_access."
                    .into()
            };
            format!("Denied by bssh: {clean}. Nothing ran.\n{next}")
        }
        ToolError::Invalid(msg) => {
            let clean = msg.trim_end_matches('.');
            let next = if clean.contains("unknown host") || clean.contains("list_hosts") {
                "Use list_hosts to see the hosts you can reach."
            } else if clean.contains("cleanly") {
                "Read the file with read_file to check its current content, then generate a patch against that."
            } else if clean.contains("duration") {
                "Specify a valid duration like '30m' or '2h'."
            } else if clean.contains("scope") {
                "Provide at least one host or tag."
            } else {
                "Check the tool arguments against its schema and retry."
            };
            format!("Invalid request: {clean}. Nothing ran.\n{next}")
        }
        ToolError::Failed(msg) => {
            let clean = msg.trim_end_matches('.');
            let next = if clean.contains("back up") {
                "Check file permissions on the host and retry."
            } else if clean.contains("timed out") {
                "The operation timed out. Check host connectivity or retry with narrower scope."
            } else {
                "Check host connectivity or run status and retry."
            };
            format!("Failed: {clean}. Nothing ran.\n{next}")
        }
        ToolError::FailedAfterRun(msg) => {
            let clean = msg.trim_end_matches('.');
            let next = if clean.contains("timed out") {
                "The command ran until the timeout; check the host before retrying."
            } else if clean.contains("does not match") || clean.contains("use undo") {
                "The write happened: undo the change, or check the host before retrying."
            } else if clean.contains("back up") {
                "Check file permissions on the host before retrying."
            } else {
                "The command ran; check the host or the reported output before retrying."
            };
            format!("Failed after running: {clean}.\n{next}")
        }
    }
}
