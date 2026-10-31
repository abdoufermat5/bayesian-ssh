//! `run_many`: one approved read-only command fanned out over tagged hosts.

use super::convert::to_value;
use super::exec::{check_command, exec_out};
use super::inputs::RunManyArgs;
use super::outputs::{RunManyEntry, RunManyOut};
use crate::cli::utils::MAX_PARALLEL_HOSTS;
use crate::models::Connection;
use crate::services::ai::mcp::context::{RunContext, ToolError, ToolResult, failed};
use crate::services::ai::mcp::server::{current_progress_token, with_progress_token};
use crate::services::ai::types::{Enforcement, ExecLimits, HostFacts, JailKind, ToolName};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(super) async fn run_many(ctx: &Arc<RunContext>, a: RunManyArgs) -> ToolResult {
    check_command(&a.command)?;
    let hosts: Vec<Connection> = ctx
        .visible_hosts()
        .await?
        .into_iter()
        .filter(|c| c.tags.iter().any(|t| t.eq_ignore_ascii_case(&a.tag)))
        .collect();
    if hosts.is_empty() {
        return Err(ToolError::Invalid(format!(
            "no hosts with tag '{}' in your scope",
            a.tag
        )));
    }
    let analysis = ctx.deps.analyzer.analyze(&a.command);
    // One decision for the whole call, over the hosts that will actually be touched. The
    // safest host decides what is safe for all of them, and a human is asked once.
    let mut facts_per_host = Vec::with_capacity(hosts.len());
    for conn in &hosts {
        facts_per_host.push(ctx.facts(conn).await);
    }
    let facts = aggregate_facts(&facts_per_host);
    let names: Vec<&str> = hosts.iter().map(|c| c.name.as_str()).collect();
    let call = ctx.new_call(
        ToolName::RunMany,
        None,
        Some(a.command.clone()),
        None,
        json!({ "tag": a.tag, "hosts": names, "command": a.command }),
    );
    let unenforced = facts.enforcement.jail == JailKind::None && !analysis.is_read_only();
    let force = unenforced.then_some(
        "analyzer: not provably read-only, and at least one host has no read-only jail to \
         stop writes",
    );
    ctx.authorize_or_ask(&call, Some(&analysis), &facts, force)
        .await?;

    let sem = Arc::new(Semaphore::new(MAX_PARALLEL_HOSTS));
    let progress = current_progress_token();
    let mut set = tokio::task::JoinSet::new();
    for conn in hosts {
        let ctx = ctx.clone();
        let sem = sem.clone();
        let command = a.command.clone();
        let progress = progress.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            let r = with_progress_token(progress, run_many_on(&ctx, &conn, &command)).await;
            (conn.name.clone(), r)
        });
    }
    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((host, Ok(v))) => results.push(RunManyEntry {
                host,
                ok: true,
                error: None,
                result: Some(v),
            }),
            Ok((host, Err(e))) => results.push(RunManyEntry {
                host,
                ok: false,
                error: Some(e.to_string()),
                result: None,
            }),
            Err(e) => return Err(failed(format!("run_many task: {e}"))),
        }
    }
    results.sort_by(|a, b| a.host.cmp(&b.host));
    let count = results.len();
    to_value(RunManyOut { results, count })
}

/// One host of an already-approved `run_many`: no second policy decision, so a host that
/// fails is a per-host result, never a new denial.
async fn run_many_on(ctx: &Arc<RunContext>, conn: &Connection, command: &str) -> ToolResult {
    let facts = ctx.facts(conn).await;
    let mode = RunContext::read_mode(&facts);
    let jailed = mode.is_jailed();
    let call = ctx.new_call(
        ToolName::RunMany,
        Some(&conn.name),
        Some(command.to_string()),
        None,
        json!({ "host": conn.name, "command": command }),
    );
    let ex = ctx
        .exec(conn, &call, command, mode, ExecLimits::default())
        .await?;
    to_value(exec_out(&conn.name, &ex, jailed))
}

/// The facts of several hosts as one: a jail/certificate counts only when every host has it,
/// and the tags are the union (a stricter tag anywhere applies to the whole call).
fn aggregate_facts(facts: &[HostFacts]) -> HostFacts {
    let mut tags: Vec<String> = Vec::new();
    for f in facts {
        for t in &f.tags {
            if !tags.contains(t) {
                tags.push(t.clone());
            }
        }
    }
    let all = |f: &dyn Fn(&HostFacts) -> bool| !facts.is_empty() && facts.iter().all(f);
    let jail = match facts.first().map(|f| f.enforcement.jail) {
        Some(k) if facts.iter().all(|f| f.enforcement.jail == k) => k,
        _ => JailKind::None,
    };
    HostFacts {
        name: String::new(),
        tags,
        tags_trusted: all(&|f| f.tags_trusted),
        tag_source: None,
        enforcement: Enforcement {
            jail,
            certificates: all(&|f| f.enforcement.certificates),
            agent_user: all(&|f| f.enforcement.agent_user),
        },
        probe_error: facts.iter().find_map(|f| f.probe_error.clone()),
    }
}
