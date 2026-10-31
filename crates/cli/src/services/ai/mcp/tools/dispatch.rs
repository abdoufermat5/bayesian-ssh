//! Running one tool call through the pipeline, with asynchronous approvals.

use super::convert::args;
use super::exec::{run_readonly_on, run_tool, service};
use super::hosts::{host_info, list_hosts};
use super::inputs::{ChangeIdArgs, EditFileArgs, RunArgs};
use super::names::parse_tool;
use super::read::{observe_tool, read_file, search_logs};
use super::run_many::run_many;
use super::run_state::{request_access, status, wait_approval};
use crate::services::ai::mcp::context::{
    failed, with_call_slot, CallPhase, CallSlot, RunContext, ToolError, ToolResult,
};
use crate::services::ai::plan;
use crate::services::ai::types::ToolName;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// How long `wait_approval` holds the request open before answering `pending` again.
pub(super) const WAIT_APPROVAL_MAX: Duration = Duration::from_secs(50);

/// Run one tool call through the pipeline. `arguments` is the JSON the agent sent.
///
/// With asynchronous approvals a call that needs a human returns `{status:"pending"}`
/// immediately while the handler keeps running in the background; the approved action
/// still executes exactly once, and `wait_approval` returns its result.
pub async fn call(ctx: &Arc<RunContext>, name: &str, arguments: Value) -> ToolResult {
    let tool =
        parse_tool(name).ok_or_else(|| ToolError::Invalid(format!("unknown tool '{name}'")))?;
    // `status` explains a pause and `wait_approval` collects results from before it; neither
    // performs a new action, so neither is blocked by the breaker or charged to the budget.
    if matches!(tool, ToolName::Status | ToolName::WaitApproval) {
        return dispatch(ctx, tool, arguments).await;
    }
    ctx.check_alive()?;
    ctx.rate_limit()?;
    if !ctx.async_approvals() {
        return dispatch(ctx, tool, arguments).await;
    }
    let slot = CallSlot::new(name);
    let handler_ctx = ctx.clone();
    let fut = with_call_slot(slot.clone(), async move {
        dispatch(&handler_ctx, tool, arguments).await
    });
    drive_call(slot, fut).await
}

/// Poll the handler until it needs a human or finishes. When it asks, the handler keeps
/// running in the background and the agent gets `pending`.
async fn drive_call<F>(slot: Arc<CallSlot>, fut: F) -> ToolResult
where
    F: Future<Output = ToolResult> + Send + 'static,
{
    let mut fut = Box::pin(fut);
    let mut phase = slot.subscribe();
    loop {
        match slot.phase_now() {
            CallPhase::Done(r) => return r,
            CallPhase::Pending {
                request_id,
                expires_in_secs,
                ..
            } => {
                let tool = slot.tool().to_string();
                tokio::spawn(async move {
                    let result = fut.await;
                    slot.finish(result);
                });
                return Ok(pending_value(&tool, &request_id, expires_in_secs));
            }
            CallPhase::Running => {}
        }
        tokio::select! {
            r = &mut fut => return r,
            changed = phase.changed() => {
                if changed.is_err() {
                    return Err(failed("the call's approval state was lost"));
                }
            }
        }
    }
}

fn pending_value(tool: &str, request_id: &str, expires_in_secs: u64) -> Value {
    json!({
        "status": "pending",
        "request_id": request_id,
        "expires_in_secs": expires_in_secs,
        "tool": tool,
        "approve_with": "the bssh desktop app or `bssh ai watch`",
        "next": "call wait_approval with this request_id",
    })
}

async fn dispatch(ctx: &Arc<RunContext>, tool: ToolName, arguments: Value) -> ToolResult {
    match tool {
        ToolName::ListHosts => list_hosts(ctx, args(arguments)?).await,
        ToolName::HostInfo => host_info(ctx, args(arguments)?).await,
        ToolName::Observe => observe_tool(ctx, args(arguments)?).await,
        ToolName::ReadFile => read_file(ctx, args(arguments)?).await,
        ToolName::SearchLogs => search_logs(ctx, args(arguments)?).await,
        ToolName::RunReadonly => {
            let a: RunArgs = args(arguments)?;
            let conn = ctx.resolve_host(&a.host).await?;
            run_readonly_on(ctx, &conn, &a.command, ToolName::RunReadonly).await
        }
        ToolName::RunMany => run_many(ctx, args(arguments)?).await,
        ToolName::ProposeChange => plan::propose(ctx, args(arguments)?).await,
        ToolName::ApplyChange => {
            let a: ChangeIdArgs = args(arguments)?;
            plan::apply(ctx, &a.change_id).await
        }
        ToolName::EditFile => {
            let a: EditFileArgs = args(arguments)?;
            plan::edit_file(ctx, &a.host, &a.path, &a.patch).await
        }
        ToolName::Service => service(ctx, args(arguments)?).await,
        ToolName::Run => run_tool(ctx, args(arguments)?).await,
        ToolName::Undo => {
            let a: ChangeIdArgs = args(arguments)?;
            plan::undo(ctx, &a.change_id).await
        }
        ToolName::Status => status(ctx, args(arguments)?).await,
        ToolName::RequestAccess => request_access(ctx, args(arguments)?).await,
        ToolName::WaitApproval => wait_approval(ctx, args(arguments)?).await,
    }
}
