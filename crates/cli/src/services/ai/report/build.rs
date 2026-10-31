//! Builds a run report from audit entries and the change records on disk.

use crate::config::AppConfig;
use crate::services::ai::audit::{AuditEntry, read_entries};
use crate::services::ai::plan::{ChangeRecord, PlanState, StepRecord};
use crate::services::ai::types::{
    AiEvent, ApprovalOutcome, ChangePlan, Decision, Redaction, Step, StepAction,
};
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeSet;

use super::casts::run_casts;
use super::model::{ReportApproval, ReportCall, ReportChange, ReportStep, RunReport};

fn event_run_id(e: &AiEvent) -> &str {
    match e {
        AiEvent::RunStarted { run_id, .. }
        | AiEvent::Approval { run_id, .. }
        | AiEvent::Result { run_id, .. }
        | AiEvent::Change { run_id, .. }
        | AiEvent::Paused { run_id, .. }
        | AiEvent::Resumed { run_id, .. }
        | AiEvent::RunEnded { run_id, .. } => run_id,
        AiEvent::Call { call, .. } => &call.run_id,
    }
}

fn tool_str(t: crate::services::ai::types::ToolName) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// Distinct run ids in first-seen order.
pub(super) fn run_ids(entries: &[AuditEntry]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for e in entries {
        let r = event_run_id(&e.event);
        if seen.insert(r.to_string()) {
            out.push(r.to_string());
        }
    }
    out
}

/// Resolve a full id or a unique prefix.
pub fn resolve_run_id(entries: &[AuditEntry], wanted: &str) -> Result<String> {
    let ids = run_ids(entries);
    if ids.iter().any(|i| i == wanted) {
        return Ok(wanted.to_string());
    }
    let m: Vec<&String> = ids.iter().filter(|i| i.starts_with(wanted)).collect();
    match m.len() {
        0 => anyhow::bail!("no run '{wanted}' in the audit log"),
        1 => Ok(m[0].clone()),
        _ => anyhow::bail!("run id '{wanted}' is ambiguous"),
    }
}

/// Build the report of one run from entries already read.
pub fn build_report(entries: &[AuditEntry], run_id: &str) -> RunReport {
    let mut r = RunReport {
        run_id: run_id.to_string(),
        client: None,
        on_behalf_of: None,
        policy: None,
        policy_hash: None,
        started: None,
        ended: None,
        end_reason: None,
        hosts: Vec::new(),
        calls: Vec::new(),
        approvals: Vec::new(),
        changes: Vec::new(),
        denials: Vec::new(),
        redactions: Vec::new(),
        circuit_breaker: Vec::new(),
        recordings: Vec::new(),
    };
    let mut hosts = BTreeSet::new();
    let mut reds: std::collections::BTreeMap<String, usize> = Default::default();
    for e in entries.iter().filter(|e| event_run_id(&e.event) == run_id) {
        match &e.event {
            AiEvent::RunStarted {
                client,
                on_behalf_of,
                policy,
                policy_hash,
                ..
            } => {
                r.client = Some(client.clone());
                r.on_behalf_of = Some(on_behalf_of.clone());
                r.policy = Some(policy.clone());
                r.policy_hash = Some(policy_hash.clone());
                r.started = Some(e.ts.clone());
            }
            AiEvent::Call { call, decision } => {
                if let Some(h) = &call.host {
                    hosts.insert(h.clone());
                }
                let (d, reason) = match decision {
                    Decision::Allow { reason } => ("allow", reason),
                    Decision::Ask { reason, .. } => ("ask", reason),
                    Decision::Deny { reason } => ("deny", reason),
                };
                if d == "deny" {
                    r.denials.push(format!(
                        "{} {}: {}",
                        tool_str(call.tool),
                        call.command
                            .as_deref()
                            .or(call.path.as_deref())
                            .unwrap_or(""),
                        reason
                    ));
                }
                r.calls.push(ReportCall {
                    seq: e.seq,
                    ts: e.ts.clone(),
                    call_id: call.call_id.clone(),
                    tool: tool_str(call.tool),
                    host: call.host.clone(),
                    command: call.command.clone(),
                    path: call.path.clone(),
                    decision: d.into(),
                    reason: reason.clone(),
                    exit_code: None,
                    output_sha256: None,
                    redactions: Vec::new(),
                });
            }
            AiEvent::Approval {
                request_id,
                outcome,
                ..
            } => {
                let (o, by, channel, reason) = match outcome {
                    ApprovalOutcome::Approved { by, channel, .. } => {
                        ("approved", Some(by.clone()), Some(channel.clone()), None)
                    }
                    ApprovalOutcome::Denied { by, reason } => {
                        ("denied", Some(by.clone()), None, reason.clone())
                    }
                    ApprovalOutcome::Expired => ("expired", None, None, None),
                };
                r.approvals.push(ReportApproval {
                    seq: e.seq,
                    ts: e.ts.clone(),
                    request_id: request_id.clone(),
                    outcome: o.into(),
                    by,
                    channel,
                    reason,
                });
            }
            AiEvent::Result {
                call_id,
                exit_code,
                output_sha256,
                redactions,
                ..
            } => {
                for x in redactions {
                    *reds.entry(x.kind.clone()).or_default() += x.count;
                }
                if let Some(c) = r.calls.iter_mut().rev().find(|c| &c.call_id == call_id) {
                    c.exit_code = *exit_code;
                    c.output_sha256 = Some(output_sha256.clone());
                    c.redactions = redactions.clone();
                }
            }
            AiEvent::Change {
                change_id,
                plan_hash,
                applied,
                undo_point,
                ..
            } => r.changes.push(ReportChange {
                seq: e.seq,
                ts: e.ts.clone(),
                change_id: change_id.clone(),
                plan_hash: plan_hash.clone(),
                applied: *applied,
                undo_point: undo_point.clone(),
                title: None,
                reason: None,
                approved_by: None,
                approved_channel: None,
                approved_at: None,
                steps_ok: None,
                checks_ok: None,
                undone: false,
                steps: Vec::new(),
            }),
            AiEvent::Paused { reason, .. } => r.circuit_breaker.push(reason.clone()),
            AiEvent::Resumed { .. } => {}
            AiEvent::RunEnded { reason, .. } => {
                r.ended = Some(e.ts.clone());
                r.end_reason = Some(reason.clone());
            }
        }
    }
    r.hosts = hosts.into_iter().collect();
    r.redactions = reds
        .into_iter()
        .map(|(kind, count)| Redaction { kind, count })
        .collect();
    for c in &mut r.changes {
        if let Some(app) = r.approvals.iter().find(|a| a.outcome == "approved") {
            c.approved_by = app.by.clone();
            c.approved_channel = app.channel.clone();
            c.approved_at = Some(app.ts.clone());
        }
    }
    r
}

/// Report of one run (full id or unique prefix) from the local audit log.
pub fn run_report(config: &AppConfig, run_id: &str) -> Result<RunReport> {
    let entries = read_entries(config)?;
    let id = resolve_run_id(&entries, run_id)?;
    let mut r = build_report(&entries, &id);
    r.recordings = run_casts(config, &id)
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    enrich_changes_from_disk(config, &id, &mut r.changes, &r.approvals);
    Ok(r)
}

/// `<ai_dir>/changes/<run>/<change>.json`, as `plan::persist_record` writes it.
#[derive(Deserialize)]
struct StoredChange {
    plan: Option<ChangePlan>,
    approved_by: Option<String>,
    state: Option<PlanState>,
    #[serde(default)]
    record: ChangeRecord,
}

fn step_action(action: &StepAction) -> String {
    match action {
        StepAction::Run { command } => command.clone(),
        StepAction::EditFile { path, .. } => format!("edit {path}"),
        StepAction::Service { unit, action } => {
            let verb = serde_json::to_value(action)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            format!("{verb} {unit}")
        }
        StepAction::Undo { step } => format!("undo {step}"),
    }
}

fn report_step(phase: &str, planned: Option<&Step>, ran: Option<&StepRecord>) -> ReportStep {
    ReportStep {
        phase: phase.into(),
        id: planned
            .map(|s| s.id.clone())
            .or_else(|| ran.map(|r| r.id.clone()))
            .unwrap_or_default(),
        host: planned
            .and_then(|s| s.host.clone())
            .or_else(|| ran.and_then(|r| r.host.clone())),
        action: planned
            .map(|s| step_action(&s.action))
            .or_else(|| ran.map(|r| r.kind.clone()))
            .unwrap_or_default(),
        ok: ran.map(|r| r.ok),
        exit_code: ran.and_then(|r| r.exit_code),
        error: ran.and_then(|r| r.error.clone()),
        output: ran
            .map(|r| r.untrusted_output.clone())
            .filter(|o| !o.trim().is_empty()),
    }
}

/// Planned steps in plan order, each with its record when it ran.
fn report_steps(phase: &str, planned: &[Step], ran: &[StepRecord]) -> Vec<ReportStep> {
    planned
        .iter()
        .map(|s| report_step(phase, Some(s), ran.iter().find(|r| r.id == s.id)))
        .collect()
}

fn ok_count(records: &[StepRecord]) -> (usize, usize) {
    (records.iter().filter(|s| s.ok).count(), records.len())
}

fn enrich_changes_from_disk(
    config: &AppConfig,
    run_id: &str,
    changes: &mut [ReportChange],
    approvals: &[ReportApproval],
) {
    let changes_dir = crate::services::ai::ai_dir(config)
        .join("changes")
        .join(run_id);
    for c in changes.iter_mut() {
        let path = changes_dir.join(format!("{}.json", c.change_id));
        let stored = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice::<StoredChange>(&data).ok());
        if let Some(s) = stored {
            let rec = &s.record;
            if let Some(plan) = &s.plan {
                c.title = Some(plan.title.clone());
                c.reason = Some(plan.reason.clone()).filter(|r| !r.trim().is_empty());
                c.steps = report_steps("step", &plan.steps, &rec.steps);
                c.steps
                    .extend(report_steps("verify", &plan.verify, &rec.verify));
                c.steps.extend(rec.undo.iter().map(|r| {
                    report_step("undo", plan.rollback.iter().find(|s| s.id == r.id), Some(r))
                }));
            }
            if let Some(by) = s.approved_by {
                c.approved_by = Some(by);
            }
            c.undone = s.state == Some(PlanState::Undone);
            if !rec.steps.is_empty() {
                c.steps_ok = Some(ok_count(&rec.steps));
            }
            if !rec.verify.is_empty() {
                c.checks_ok = Some(ok_count(&rec.verify));
            }
        }
        if c.approved_by.is_none() {
            if let Some(app) = approvals.iter().find(|a| a.outcome == "approved") {
                c.approved_by = app.by.clone();
                c.approved_channel = app.channel.clone();
                c.approved_at = Some(app.ts.clone());
            }
        } else if c.approved_at.is_none()
            && let Some(app) = approvals.iter().find(|a| a.outcome == "approved")
        {
            if c.approved_channel.is_none() {
                c.approved_channel = app.channel.clone();
            }
            c.approved_at = Some(app.ts.clone());
        }
    }
}
