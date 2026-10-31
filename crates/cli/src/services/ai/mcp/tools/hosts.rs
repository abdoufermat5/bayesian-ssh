//! `list_hosts` and `host_info`.

use super::convert::{exit_text, to_value};
use super::inputs::{HostArgs, ListHostsArgs};
use super::outputs::{EnforcementOut, HostEntry, HostInfoOut, HostListOut, NOTICE};
use crate::models::Connection;
use crate::services::ai::mcp::context::{RunContext, ToolResult};
use crate::services::ai::types::{ExecLimits, HostFacts, ToolName};
use serde_json::json;
use std::sync::Arc;

fn host_entry(c: &Connection) -> HostEntry {
    HostEntry {
        name: c.name.clone(),
        host: c.host.clone(),
        user: c.user.clone(),
        port: c.port,
        tags: c.tags.clone(),
        aliases: c.aliases.clone(),
        via_bastion: c.bastion.is_some(),
        bastion: c.bastion.clone(),
    }
}

pub(super) async fn list_hosts(ctx: &Arc<RunContext>, a: ListHostsArgs) -> ToolResult {
    let call = ctx.new_call(
        ToolName::ListHosts,
        None,
        None,
        None,
        json!({ "tag": a.tag, "query": a.query }),
    );
    ctx.authorize(&call, None, &HostFacts::default()).await?;
    let q = a.query.as_deref().map(str::to_lowercase);
    let hosts: Vec<HostEntry> = ctx
        .visible_hosts()
        .await?
        .iter()
        .filter(|c| {
            a.tag
                .as_deref()
                .map(|t| c.tags.iter().any(|ct| ct.eq_ignore_ascii_case(t)))
                .unwrap_or(true)
        })
        .filter(|c| {
            q.as_deref()
                .map(|q| {
                    c.name.to_lowercase().contains(q)
                        || c.host.to_lowercase().contains(q)
                        || c.tags.iter().any(|t| t.to_lowercase().contains(q))
                        || c.aliases.iter().any(|t| t.to_lowercase().contains(q))
                })
                .unwrap_or(true)
        })
        .map(host_entry)
        .collect();
    let count = hosts.len();
    to_value(HostListOut { hosts, count })
}

pub(super) async fn host_info(ctx: &Arc<RunContext>, a: HostArgs) -> ToolResult {
    let conn = ctx.resolve_host(&a.host).await?;
    let facts = ctx.facts(&conn).await;
    // Fixed command: not agent text, so no analysis.
    let cmd = "uname -sr; uptime -p 2>/dev/null || uptime";
    let call = ctx.new_call(
        ToolName::HostInfo,
        Some(&conn.name),
        Some(cmd.into()),
        None,
        json!({ "host": a.host }),
    );
    ctx.authorize(&call, None, &facts).await?;
    let (system, remote_error) = match ctx
        .exec(
            &conn,
            &call,
            cmd,
            RunContext::read_mode(&facts),
            ExecLimits::default(),
        )
        .await
    {
        Ok(ex) if ex.result.exit_code == Some(0) => {
            let mut lines = ex.result.stdout.lines();
            (
                Some(json!({
                    "kernel": lines.next().unwrap_or(""),
                    "uptime": lines.next().unwrap_or(""),
                })),
                None,
            )
        }
        Ok(ex) => (None, Some(exit_text(ex.result.exit_code))),
        Err(e) => (None, Some(e.to_string())),
    };
    let e = &facts.enforcement;
    to_value(HostInfoOut {
        name: conn.name.clone(),
        host: conn.host.clone(),
        user: conn.user.clone(),
        port: conn.port,
        tags: conn.tags.clone(),
        aliases: conn.aliases.clone(),
        via_bastion: conn.bastion.is_some(),
        enforcement: EnforcementOut {
            jail: serde_json::to_value(e.jail)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "none".into()),
            certificates: e.certificates,
            agent_user: e.agent_user,
        },
        untrusted_system: system,
        remote_error,
        probe_error: facts.probe_error.clone(),
        notice: NOTICE.into(),
    })
}
