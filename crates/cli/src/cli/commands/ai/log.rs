//! `bssh ai log`.

use crate::cli::parser_ai::AiArgsLog;
use crate::config::AppConfig;
use crate::services::ai::{audit, report};
use anyhow::Result;

pub async fn execute(args: AiArgsLog, config: AppConfig) -> Result<()> {
    let Some(run) = args.run else {
        let runs = report::list_runs(&config, args.limit)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&runs)?);
        } else if runs.is_empty() {
            println!("No agent runs recorded yet.");
        } else {
            for r in runs {
                println!(
                    "{}  {}  client={} policy={} calls={} denials={} changes={}{}",
                    r.run_id,
                    r.started.as_deref().unwrap_or("-"),
                    r.client.as_deref().unwrap_or("-"),
                    r.policy.as_deref().unwrap_or("-"),
                    r.calls,
                    r.denials,
                    r.changes,
                    if r.ended.is_some() { "" } else { "  (open)" }
                );
            }
        }
        return Ok(());
    };

    let entries = audit::read_entries(&config)?;
    let id = report::resolve_run_id(&entries, &run)?;
    if args.report {
        let r = report::run_report(&config, &id)?;
        if args.json {
            println!("{}", r.to_json()?);
        } else {
            print!("{}", r.to_text());
        }
        return Ok(());
    }

    let events: Vec<_> = entries
        .iter()
        .filter(|e| event_run(&e.event) == id)
        .take(args.limit.max(1) * 1000)
        .collect();
    if args.json {
        let v: Vec<_> = events
            .iter()
            .map(|e| serde_json::json!({"seq": e.seq, "ts": e.ts, "event": e.event}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for e in events {
            println!("#{} {} {}", e.seq, e.ts, one_line(&e.event));
        }
    }
    Ok(())
}

fn event_run(e: &crate::services::ai::types::AiEvent) -> String {
    use crate::services::ai::types::AiEvent::*;
    match e {
        RunStarted { run_id, .. }
        | Approval { run_id, .. }
        | Result { run_id, .. }
        | Change { run_id, .. }
        | CircuitBreaker { run_id, .. }
        | RunEnded { run_id, .. } => run_id.clone(),
        Call { call, .. } => call.run_id.clone(),
    }
}

fn one_line(e: &crate::services::ai::types::AiEvent) -> String {
    use crate::services::ai::types::{AiEvent::*, ApprovalOutcome, Decision};
    match e {
        RunStarted {
            client,
            on_behalf_of,
            policy,
            ..
        } => format!("run started: {client} on behalf of {on_behalf_of}, policy {policy}"),
        Call { call, decision } => {
            let d = match decision {
                Decision::Allow { reason } => format!("allow ({reason})"),
                Decision::Ask { reason, .. } => format!("ask ({reason})"),
                Decision::Deny { reason } => format!("deny ({reason})"),
            };
            format!(
                "call {} {} {} -> {}",
                call.call_id,
                call.host.as_deref().unwrap_or("-"),
                call.command
                    .as_deref()
                    .or(call.path.as_deref())
                    .unwrap_or(""),
                d
            )
        }
        Approval {
            request_id,
            outcome,
            ..
        } => match outcome {
            ApprovalOutcome::Approved { by, channel, .. } => {
                format!("approval {request_id}: approved by {by} via {channel}")
            }
            ApprovalOutcome::Denied { by, .. } => format!("approval {request_id}: denied by {by}"),
            ApprovalOutcome::Expired => format!("approval {request_id}: expired"),
        },
        Result {
            call_id, exit_code, ..
        } => format!("result {call_id}: exit {exit_code:?}"),
        Change {
            change_id,
            plan_hash,
            applied,
            undo_point,
            ..
        } => format!(
            "change {change_id} plan {plan_hash} applied={applied} undo={}",
            undo_point.as_deref().unwrap_or("-")
        ),
        CircuitBreaker { reason, .. } => format!("circuit breaker: {reason}"),
        RunEnded { reason, .. } => format!("run ended: {reason}"),
    }
}
