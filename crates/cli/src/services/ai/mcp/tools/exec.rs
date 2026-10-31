//! Command tools: `run_readonly`, `run` and `service`.

use super::convert::{reds, to_value};
use super::inputs::{RunArgs, ServiceArgs};
use super::outputs::{ExecOut, NOTICE, ServiceOut};
use crate::models::Connection;
use crate::services::ai::mcp::context::{Executed, RunContext, ToolError, ToolResult};
use crate::services::ai::mcp::util::valid_unit;
use crate::services::ai::plan;
use crate::services::ai::types::{ExecLimits, JailKind, ServiceAction, ToolName};
use serde_json::json;
use std::sync::Arc;

/// Longest command accepted by `run`, `run_readonly` and `run_many`, before any analysis.
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;

pub(super) fn exec_out(host: &str, ex: &Executed, jailed: bool) -> ExecOut {
    ExecOut {
        host: host.to_string(),
        exit_code: ex.result.exit_code,
        timed_out: ex.result.timed_out,
        truncated: ex.result.truncated,
        duration_ms: ex.result.duration_ms,
        jailed,
        untrusted_output: ex.result.stdout.clone(),
        untrusted_stderr: ex.result.stderr.clone(),
        redactions: reds(&ex.redactions),
        notice: NOTICE.into(),
    }
}

/// Command tools take text from an agent: refuse anything longer than [`MAX_COMMAND_BYTES`]
/// before it reaches the analyzer or a glob comparison.
pub(super) fn check_command(command: &str) -> Result<(), ToolError> {
    if command.trim().is_empty() {
        return Err(ToolError::Invalid("empty command".into()));
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(ToolError::Invalid(format!(
            "command is longer than {MAX_COMMAND_BYTES} bytes"
        )));
    }
    Ok(())
}

/// `run_readonly` on one host (also the unit of `run_many`).
pub async fn run_readonly_on(
    ctx: &Arc<RunContext>,
    conn: &Connection,
    command: &str,
    tool: ToolName,
) -> ToolResult {
    check_command(command)?;
    let facts = ctx.facts(conn).await;
    let analysis = ctx.deps.analyzer.analyze(command);
    let call = ctx.new_call(
        tool,
        Some(&conn.name),
        Some(command.to_string()),
        None,
        json!({ "host": conn.name, "command": command }),
    );
    // Without a jail an unanalysed command has no automatic path: the policy may allow it,
    // but a human still approves the exact tool, host and command.
    let unenforced = facts.enforcement.jail == JailKind::None && !analysis.is_read_only();
    let force = unenforced.then_some(
        "analyzer: not provably read-only, and the host has no read-only jail to stop writes",
    );
    ctx.authorize_or_ask(&call, Some(&analysis), &facts, force)
        .await?;
    let mode = RunContext::read_mode(&facts);
    let jailed = mode.is_jailed();
    let ex = ctx
        .exec(conn, &call, command, mode, ExecLimits::default())
        .await?;
    to_value(exec_out(&conn.name, &ex, jailed))
}

pub(super) async fn run_tool(ctx: &Arc<RunContext>, a: RunArgs) -> ToolResult {
    check_command(&a.command)?;
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    let analysis = ctx.deps.analyzer.analyze(&a.command);
    let call = ctx.new_call(
        ToolName::Run,
        Some(&conn.name),
        Some(a.command.clone()),
        None,
        json!({ "host": conn.name, "command": a.command }),
    );
    // Before asking: a human must never approve work the budget then refuses.
    ctx.consume_change()?;
    ctx.authorize(&call, Some(&analysis), &facts).await?;
    let ex = ctx
        .exec(
            &conn,
            &call,
            &a.command,
            RunContext::write_mode(&facts),
            ExecLimits::default(),
        )
        .await?;
    to_value(exec_out(&conn.name, &ex, false))
}

pub(super) async fn service(ctx: &Arc<RunContext>, a: ServiceArgs) -> ToolResult {
    if !valid_unit(&a.unit) {
        return Err(ToolError::Invalid(format!(
            "invalid unit name '{}'",
            a.unit
        )));
    }
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    let action: ServiceAction = a.action.into();
    let cmd = plan::service_command(&a.unit, action, conn.user != "root");
    let analysis = ctx.deps.analyzer.analyze(&cmd);
    let call = ctx.new_call(
        ToolName::Service,
        Some(&conn.name),
        Some(cmd.clone()),
        None,
        json!({ "host": conn.name, "unit": a.unit, "action": plan::action_verb(action) }),
    );
    // Before asking: a human must never approve work the budget then refuses.
    ctx.consume_change()?;
    ctx.authorize(&call, Some(&analysis), &facts).await?;
    let ex = ctx
        .exec(
            &conn,
            &call,
            &cmd,
            RunContext::write_mode(&facts),
            ExecLimits::default(),
        )
        .await?;
    let ran_ok = ex.result.exit_code == Some(0) && !ex.result.timed_out;
    let healthy = if ran_ok && action != ServiceAction::Stop {
        Some(
            plan::service_healthy(ctx, &conn, &facts, &a.unit)
                .await
                .unwrap_or(false),
        )
    } else {
        None
    };
    let ok = ran_ok && healthy.unwrap_or(true);
    to_value(ServiceOut {
        host: conn.name.clone(),
        unit: a.unit.clone(),
        action: plan::action_verb(action).into(),
        status: if ok { "ok" } else { "failed" }.into(),
        healthy,
        result: to_value(exec_out(&conn.name, &ex, false))?,
    })
}
