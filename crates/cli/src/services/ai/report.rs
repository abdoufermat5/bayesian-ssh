//! Run reports, recordings of agent output, bssh ai log / replay.
//!
//! Reports are built from the audit log only, never by a model.

use super::audit::{read_entries, AuditEntry};
use super::types::{AiEvent, ApprovalOutcome, Decision, ExecResult, Redaction};
use super::ui::{self, Glyph, Style, Tone};
use crate::config::AppConfig;
use crate::services::recording::{recordings_dir, Recorder};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
// ---------------------------------------------------------------- recordings

fn safe_part(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cast_name(run_id: &str, call_id: &str) -> String {
    format!("ai-{}-{}.cast", safe_part(run_id), safe_part(call_id))
}

/// Path of the recording of one call.
pub fn cast_path(config: &AppConfig, run_id: &str, call_id: &str) -> PathBuf {
    recordings_dir(config).join(cast_name(run_id, call_id))
}

/// Recordings of a run (full run id), sorted by name.
pub fn run_casts(config: &AppConfig, run_id: &str) -> Vec<PathBuf> {
    let prefix = format!("ai-{}-", safe_part(run_id));
    let mut v: Vec<PathBuf> = std::fs::read_dir(recordings_dir(config))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".cast"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

/// Drop `ai-*.cast` files older than `recording.retention_days` (0 = keep forever).
fn prune_ai_casts(config: &AppConfig) {
    let days = config.recording.retention_days;
    if days == 0 {
        return;
    }
    let Some(cutoff) = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(u64::from(days) * 86_400))
    else {
        return;
    };
    for e in std::fs::read_dir(recordings_dir(config))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("ai-") && name.ends_with(".cast")) {
            continue;
        }
        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
            if m < cutoff {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Store the output of one call as an asciicast recording when `ai.record_output` is on.
pub fn record_output(
    config: &AppConfig,
    run_id: &str,
    call_id: &str,
    command: &str,
    result: &ExecResult,
) -> anyhow::Result<()> {
    if !config.ai.record_output {
        return Ok(());
    }
    prune_ai_casts(config);
    let path = cast_path(config, run_id, call_id);
    let title = format!("ai {run_id} {call_id}: {command}");
    let mut rec = Recorder::create(
        &path,
        80,
        24,
        &title,
        config.recording.max_bytes_per_session.max(64 * 1024),
    )?;
    let crlf = |s: &str| s.replace("\r\n", "\n").replace('\n', "\r\n");
    rec.output(&format!("$ {}\r\n", crlf(command)));
    for stream in [&result.stdout, &result.stderr] {
        if !stream.is_empty() {
            rec.output(&crlf(stream));
            if !stream.ends_with('\n') {
                rec.output("\r\n");
            }
        }
    }
    let status = if result.timed_out {
        "[timed out]".to_string()
    } else if let Some(c) = result.exit_code {
        format!("[exit {c}]")
    } else {
        "[no exit code]".to_string()
    };
    let trunc = if result.truncated {
        " [output truncated]"
    } else {
        ""
    };
    rec.output(&format!("{status}{trunc}\r\n"));
    rec.finish();
    Ok(())
}

// ---------------------------------------------------------------- reports

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCall {
    pub seq: u64,
    pub ts: String,
    pub call_id: String,
    pub tool: String,
    pub host: Option<String>,
    pub command: Option<String>,
    pub path: Option<String>,
    /// `allow`, `ask` or `deny`.
    pub decision: String,
    pub reason: String,
    pub exit_code: Option<i32>,
    pub output_sha256: Option<String>,
    pub redactions: Vec<Redaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportApproval {
    pub seq: u64,
    pub ts: String,
    pub request_id: String,
    /// `approved`, `denied` or `expired`.
    pub outcome: String,
    pub by: Option<String>,
    pub channel: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportChange {
    pub seq: u64,
    pub ts: String,
    pub change_id: String,
    pub plan_hash: String,
    pub applied: bool,
    pub undo_point: Option<String>,
    #[serde(skip_serializing, default)]
    pub title: Option<String>,
    #[serde(skip_serializing, default)]
    pub approved_by: Option<String>,
    #[serde(skip_serializing, default)]
    pub approved_channel: Option<String>,
    #[serde(skip_serializing, default)]
    pub approved_at: Option<String>,
    #[serde(skip_serializing, default)]
    pub steps_ok: Option<(usize, usize)>,
    #[serde(skip_serializing, default)]
    pub checks_ok: Option<(usize, usize)>,
    #[serde(skip_serializing, default)]
    pub undone: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub client: Option<String>,
    pub on_behalf_of: Option<String>,
    pub policy: Option<String>,
    pub policy_hash: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub end_reason: Option<String>,
    pub hosts: Vec<String>,
    pub calls: Vec<ReportCall>,
    pub approvals: Vec<ReportApproval>,
    pub changes: Vec<ReportChange>,
    /// Calls the policy denied.
    pub denials: Vec<String>,
    /// Redactions summed over the run, by kind.
    pub redactions: Vec<Redaction>,
    pub circuit_breaker: Vec<String>,
    /// Recordings (`.cast` file names) found for the run.
    pub recordings: Vec<String>,
}

fn event_run_id(e: &AiEvent) -> &str {
    match e {
        AiEvent::RunStarted { run_id, .. }
        | AiEvent::Approval { run_id, .. }
        | AiEvent::Result { run_id, .. }
        | AiEvent::Change { run_id, .. }
        | AiEvent::CircuitBreaker { run_id, .. }
        | AiEvent::RunEnded { run_id, .. } => run_id,
        AiEvent::Call { call, .. } => &call.run_id,
    }
}

fn tool_str(t: super::types::ToolName) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// Distinct run ids in first-seen order.
fn run_ids(entries: &[AuditEntry]) -> Vec<String> {
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
                approved_by: None,
                approved_channel: None,
                approved_at: None,
                steps_ok: None,
                checks_ok: None,
                undone: false,
            }),
            AiEvent::CircuitBreaker { reason, .. } => r.circuit_breaker.push(reason.clone()),
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
        if let Ok(data) = std::fs::read(&path) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                if let Some(title) = v["plan"]["title"].as_str() {
                    c.title = Some(title.to_string());
                }
                if let Some(by) = v["approved_by"].as_str() {
                    c.approved_by = Some(by.to_string());
                }
                if v["state"].as_str() == Some("undone") {
                    c.undone = true;
                }
                if let Some(steps) = v["record"]["steps"].as_array() {
                    let ok_count = steps
                        .iter()
                        .filter(|s| s["ok"].as_bool() == Some(true))
                        .count();
                    c.steps_ok = Some((ok_count, steps.len()));
                }
                if let Some(verify) = v["record"]["verify"].as_array() {
                    let ok_count = verify
                        .iter()
                        .filter(|s| s["ok"].as_bool() == Some(true))
                        .count();
                    c.checks_ok = Some((ok_count, verify.len()));
                }
            }
        }
        if c.approved_by.is_none() {
            if let Some(app) = approvals.iter().find(|a| a.outcome == "approved") {
                c.approved_by = app.by.clone();
                c.approved_channel = app.channel.clone();
                c.approved_at = Some(app.ts.clone());
            }
        } else if c.approved_at.is_none() {
            if let Some(app) = approvals.iter().find(|a| a.outcome == "approved") {
                if c.approved_channel.is_none() {
                    c.approved_channel = app.channel.clone();
                }
                c.approved_at = Some(app.ts.clone());
            }
        }
    }
}

impl RunReport {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn render(&self, style: &Style, now: DateTime<Utc>) -> String {
        render_report(self, style, now)
    }

    pub fn to_text(&self) -> String {
        self.render(&Style::terminal(), Utc::now())
    }
}

pub fn render_report(report: &RunReport, style: &Style, now: DateTime<Utc>) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    // 1. Header line: agent · run · on behalf of · policy
    let agent = report.client.as_deref().unwrap_or("-");
    let run_str = format!("run {}", ui::short_id(&report.run_id));
    let obo_str = format!(
        "on behalf of {}",
        report.on_behalf_of.as_deref().unwrap_or("-")
    );
    let policy_str = format!("policy {}", report.policy.as_deref().unwrap_or("-"));
    let header_facts = style.facts(&[agent, &run_str, &obo_str, &policy_str]);
    let _ = writeln!(out, "{header_facts}");

    // 2. Time line: day_clock start → clock end (duration) · hosts
    let arrow = style.glyph(Glyph::Arrow);
    let time_part = match (report.started.as_deref(), report.ended.as_deref()) {
        (Some(start), Some(end)) => {
            let start_str = ui::day_clock(start, now);
            let end_str = ui::clock(end);
            let dur_str = match (
                DateTime::parse_from_rfc3339(start),
                DateTime::parse_from_rfc3339(end),
            ) {
                (Ok(s), Ok(e)) => {
                    let ms = (e - s).num_milliseconds().max(0) as u64;
                    format!(" ({})", ui::duration_ms(ms))
                }
                _ => String::new(),
            };
            format!("{start_str} {arrow} {end_str}{dur_str}")
        }
        (Some(start), None) => {
            let start_str = ui::day_clock(start, now);
            format!("{start_str} {arrow} running")
        }
        (None, Some(end)) => {
            let end_str = ui::clock(end);
            format!("- {arrow} {end_str}")
        }
        (None, None) => "-".to_string(),
    };
    let hosts_str = if report.hosts.is_empty() {
        "no hosts in scope".to_string()
    } else {
        format!("hosts {}", report.hosts.join(", "))
    };
    let time_line = style.facts(&[&time_part, &hosts_str]);
    let _ = writeln!(out, "{time_line}");

    // 3. Totals line with colored counts
    let total_calls = report.calls.len();
    let calls_str = format!(
        "{} call{}",
        total_calls,
        if total_calls == 1 { "" } else { "s" }
    );
    let allowed_count = report
        .calls
        .iter()
        .filter(|c| c.decision == "allow")
        .count();
    let allowed_str = style.paint(Tone::Ok, &format!("{allowed_count} allowed"));
    let denied_count = report
        .calls
        .iter()
        .filter(|c| c.decision == "deny")
        .count()
        .max(report.denials.len());
    let denied_str = if denied_count > 0 {
        style.paint(Tone::Bad, &format!("{denied_count} denied"))
    } else {
        "0 denied".to_string()
    };
    let approved_count = report
        .approvals
        .iter()
        .filter(|a| a.outcome == "approved")
        .count();
    let approved_str = if approved_count > 0 {
        style.paint(Tone::Ok, &format!("{approved_count} approved"))
    } else {
        "0 approved".to_string()
    };

    let changes_summary = if report.changes.is_empty() {
        "0 changes".to_string()
    } else if report.changes.len() == 1 {
        let c = &report.changes[0];
        let state_str = if c.undone {
            ", undone"
        } else if c.applied {
            ", applied"
        } else {
            ", not applied"
        };
        let undo_str = if c.undo_point.is_some() && !c.undone {
            ", undo kept"
        } else {
            ""
        };
        format!("1 change{state_str}{undo_str}")
    } else {
        let applied = report.changes.iter().filter(|c| c.applied).count();
        let undo_kept = report
            .changes
            .iter()
            .filter(|c| c.undo_point.is_some() && !c.undone)
            .count();
        format!(
            "{} changes, {applied} applied, {undo_kept} undo kept",
            report.changes.len()
        )
    };

    let _ = writeln!(
        out,
        "{calls_str}  {allowed_str}  {denied_str}  {approved_str}   {changes_summary}"
    );

    // 4. Section CHANGES
    let _ = writeln!(out, "\n{}", style.label("CHANGES"));
    if report.changes.is_empty() {
        let _ = writeln!(out, "  none");
    } else {
        for c in &report.changes {
            let mark = if c.undone {
                style.mark(Glyph::Undo)
            } else if c.applied {
                style.mark(Glyph::Ok)
            } else {
                style.mark(Glyph::Bad)
            };
            let short_id = ui::short_id(&c.change_id);
            let title = c.title.as_deref().unwrap_or_else(|| {
                if !c.plan_hash.is_empty() {
                    &c.plan_hash
                } else {
                    "change"
                }
            });
            let title_display = if c.title.is_some() {
                title.to_string()
            } else {
                format!("plan {title}")
            };
            let _ = writeln!(out, "  {mark} {short_id}  {title_display}");

            let mut details = Vec::new();
            let approved_str;
            if let Some(by) = &c.approved_by {
                let channel_str = match &c.approved_channel {
                    Some(ch) if ch != "in-band" => format!(" via {ch}"),
                    _ => String::new(),
                };
                let at_str = match &c.approved_at {
                    Some(at) => format!(" at {}", ui::clock(at)),
                    None => String::new(),
                };
                approved_str = format!("approved by {by}{channel_str}{at_str}");
                details.push(approved_str.as_str());
            }

            let steps_str;
            if let Some((ok, total)) = c.steps_ok {
                steps_str = format!("step {ok}/{total} ok");
                details.push(&steps_str);
            }
            let checks_str;
            if let Some((ok, total)) = c.checks_ok {
                checks_str = format!("checks {ok}/{total} ok");
                details.push(&checks_str);
            }

            let undo_str;
            if c.undone {
                details.push("undone");
            } else if c.applied {
                if c.undo_point.is_some() {
                    undo_str = format!("undo with bssh ai undo {short_id}");
                    details.push(&undo_str);
                }
            } else {
                details.push("not applied");
            }

            if !details.is_empty() {
                let _ = writeln!(out, "    {}", style.facts(&details));
            }
        }
    }

    // 5. Section DENIED (time, kind in words, host, command, reason)
    let _ = writeln!(out, "\n{}", style.label("DENIED"));
    let denied_calls: Vec<&ReportCall> = report
        .calls
        .iter()
        .filter(|c| c.decision == "deny")
        .collect();
    if denied_calls.is_empty() && report.denials.is_empty() {
        let _ = writeln!(out, "  none");
    } else if !denied_calls.is_empty() {
        for c in denied_calls {
            let time = ui::clock(&c.ts);
            let kind = ui::tool_kind_in_words(&c.tool);
            let host = c.host.as_deref().unwrap_or("-");
            let cmd = c.command.as_deref().or(c.path.as_deref()).unwrap_or("");
            let _ = writeln!(out, "  {time}  {kind}  {host}  {cmd} · {}", c.reason);
        }
    } else {
        for d in &report.denials {
            let _ = writeln!(out, "  {d}");
        }
    }

    // 6. Section REDACTED (counts by kind or 'nothing')
    let _ = writeln!(out, "\n{}", style.label("REDACTED"));
    if report.redactions.is_empty() {
        let _ = writeln!(out, "  nothing");
    } else {
        for r in &report.redactions {
            let _ = writeln!(out, "  {}: {}", r.kind, r.count);
        }
    }

    // 7. Section RECORDINGS (a count and 'bssh ai replay <run> <call>' — never list the files)
    let _ = writeln!(out, "\n{}", style.label("RECORDINGS"));
    if report.recordings.is_empty() {
        let _ = writeln!(out, "  none");
    } else {
        let count = report.recordings.len();
        let count_str = format!("{} recording{}", count, if count == 1 { "" } else { "s" });
        let _ = writeln!(
            out,
            "  {} {} bssh ai replay {} <call>",
            count_str,
            style.glyph(Glyph::Dot),
            ui::short_id(&report.run_id)
        );
    }

    // 8. Section ALL CALLS ('bssh ai log <run>')
    let _ = writeln!(out, "\n{}", style.label("ALL CALLS"));
    let _ = writeln!(out, "  bssh ai log {}", ui::short_id(&report.run_id));

    if !report.circuit_breaker.is_empty() {
        let _ = writeln!(out, "\n{}", style.label("CIRCUIT BREAKER TRIPPED"));
        for cb in &report.circuit_breaker {
            let _ = writeln!(out, "  {cb}");
        }
    }

    out
}

/// One line per run for `bssh ai log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub client: Option<String>,
    pub policy: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub calls: usize,
    pub denials: usize,
    pub changes: usize,
    #[serde(skip_serializing, default)]
    pub end_reason: Option<String>,
}

/// Runs, most recent first, at most `limit`.
pub fn list_runs(config: &AppConfig, limit: usize) -> Result<Vec<RunSummary>> {
    let entries = read_entries(config)?;
    let mut v: Vec<RunSummary> = run_ids(&entries)
        .into_iter()
        .map(|id| {
            let r = build_report(&entries, &id);
            RunSummary {
                run_id: id,
                client: r.client,
                policy: r.policy,
                started: r.started,
                ended: r.ended,
                calls: r.calls.len(),
                denials: r.denials.len(),
                changes: r.changes.len(),
                end_reason: r.end_reason,
            }
        })
        .collect();
    v.reverse();
    v.truncate(limit);
    Ok(v)
}

pub fn render_runs(style: &Style, runs: &[RunSummary], now: DateTime<Utc>) -> String {
    use std::fmt::Write;
    if runs.is_empty() {
        return "No agent runs recorded yet.\n".to_string();
    }

    struct Row {
        run: String,
        agent: String,
        started: String,
        calls: String,
        denied_raw: String,
        denied_styled: String,
        changes: String,
        state: String,
    }

    let mut rows = Vec::with_capacity(runs.len());
    for r in runs {
        let run = ui::short_id(&r.run_id);
        let agent = r.client.as_deref().unwrap_or("-").to_string();
        let started = r
            .started
            .as_deref()
            .map(|s| ui::day_clock(s, now))
            .unwrap_or_else(|| "-".into());
        let calls = r.calls.to_string();
        let denied_raw = r.denials.to_string();
        let denied_styled = if r.denials > 0 {
            style.paint(Tone::Bad, &denied_raw)
        } else {
            denied_raw.clone()
        };
        let changes = r.changes.to_string();
        let state = if let Some(ended) = &r.ended {
            let t = ui::clock_hm(ended);
            match &r.end_reason {
                Some(reason) if !reason.is_empty() => format!("ended {t}, {reason}"),
                _ => format!("ended {t}"),
            }
        } else {
            format!("{} running", style.mark(Glyph::Running))
        };

        rows.push(Row {
            run,
            agent,
            started,
            calls,
            denied_raw,
            denied_styled,
            changes,
            state,
        });
    }

    let w_run = rows
        .iter()
        .map(|r| ui::display_width(&r.run))
        .max()
        .unwrap_or(0)
        .max("RUN".len());
    let w_agent = rows
        .iter()
        .map(|r| ui::display_width(&r.agent))
        .max()
        .unwrap_or(0)
        .max("AGENT".len());
    let w_started = rows
        .iter()
        .map(|r| ui::display_width(&r.started))
        .max()
        .unwrap_or(0)
        .max("STARTED".len());
    let w_calls = rows
        .iter()
        .map(|r| ui::display_width(&r.calls))
        .max()
        .unwrap_or(0)
        .max("CALLS".len());
    let w_denied = rows
        .iter()
        .map(|r| ui::display_width(&r.denied_raw))
        .max()
        .unwrap_or(0)
        .max("DENIED".len());
    let w_changes = rows
        .iter()
        .map(|r| ui::display_width(&r.changes))
        .max()
        .unwrap_or(0)
        .max("CHANGES".len());

    let header = format!(
        "{}   {}   {}   {:>w_calls$}   {:>w_denied$}   {:>w_changes$}   STATE",
        ui::pad("RUN", w_run),
        ui::pad("AGENT", w_agent),
        ui::pad("STARTED", w_started),
        "CALLS",
        "DENIED",
        "CHANGES",
    );

    let mut out = String::new();
    let _ = writeln!(out, "{}", style.paint(Tone::Dim, &header));

    for r in &rows {
        let w_d = ui::display_width(&r.denied_styled);
        let denied_padded = if w_d < w_denied {
            format!("{}{}", " ".repeat(w_denied - w_d), r.denied_styled)
        } else {
            r.denied_styled.clone()
        };

        let _ = writeln!(
            out,
            "{}   {}   {}   {:>w_calls$}   {}   {:>w_changes$}   {}",
            ui::pad(&r.run, w_run),
            ui::pad(&r.agent, w_agent),
            ui::pad(&r.started, w_started),
            r.calls,
            denied_padded,
            r.changes,
            r.state
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::audit::LocalAudit;
    use crate::services::ai::types::*;

    fn cfg(dir: &std::path::Path) -> AppConfig {
        AppConfig {
            database_path: dir.join("env").join("bssh.db"),
            ..Default::default()
        }
    }

    fn call(run: &str, id: &str, tool: ToolName, host: &str, cmd: &str) -> ToolCall {
        ToolCall {
            run_id: run.into(),
            call_id: id.into(),
            tool,
            host: Some(host.into()),
            command: Some(cmd.into()),
            path: None,
            args: serde_json::json!({}),
        }
    }

    fn fixture(c: &AppConfig) {
        let a = LocalAudit::open(c).unwrap();
        let evs = vec![
            AiEvent::RunStarted {
                run_id: "run-1".into(),
                client: "claude-code 2".into(),
                on_behalf_of: "alice".into(),
                policy: "default".into(),
                policy_hash: "ph".into(),
            },
            AiEvent::Call {
                call: call("run-1", "c1", ToolName::RunReadonly, "web1", "df -h"),
                decision: Decision::Allow {
                    reason: "read".into(),
                },
            },
            AiEvent::Result {
                duration_ms: None,
                jailed: None,
                run_id: "run-1".into(),
                call_id: "c1".into(),
                exit_code: Some(0),
                output_sha256: "abc".into(),
                redactions: vec![Redaction {
                    kind: "aws-key".into(),
                    count: 2,
                }],
            },
            AiEvent::Call {
                call: call("run-1", "c2", ToolName::Run, "db1", "shutdown now"),
                decision: Decision::Deny {
                    reason: "destructive".into(),
                },
            },
            AiEvent::Call {
                call: call("run-1", "c3", ToolName::Service, "web1", "restart nginx"),
                decision: Decision::Ask {
                    tier: Tier::OutOfBand,
                    reason: "service".into(),
                },
            },
            AiEvent::Approval {
                request_id: "req1".into(),
                run_id: "run-1".into(),
                outcome: ApprovalOutcome::Approved {
                    by: "alice".into(),
                    channel: "desktop".into(),
                    grant: None,
                },
            },
            AiEvent::Change {
                run_id: "run-1".into(),
                change_id: "chg1".into(),
                plan_hash: "deadbeef".into(),
                applied: true,
                undo_point: Some("undo-1".into()),
            },
            AiEvent::CircuitBreaker {
                run_id: "run-1".into(),
                reason: "5 denials".into(),
            },
            AiEvent::RunEnded {
                run_id: "run-1".into(),
                reason: "stopped".into(),
            },
            AiEvent::RunStarted {
                run_id: "run-2".into(),
                client: "codex".into(),
                on_behalf_of: "bob".into(),
                policy: "p".into(),
                policy_hash: "q".into(),
            },
        ];
        for e in &evs {
            a.record(e).unwrap();
        }
    }

    #[test]
    fn report_from_fixture_log() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        fixture(&c);
        let r = run_report(&c, "run-1").unwrap();
        assert_eq!(r.hosts, vec!["db1", "web1"]);
        assert_eq!(r.calls.len(), 3);
        assert_eq!(r.calls[0].exit_code, Some(0));
        assert_eq!(r.denials.len(), 1);
        assert_eq!(r.approvals[0].by.as_deref(), Some("alice"));
        assert_eq!(r.approvals[0].channel.as_deref(), Some("desktop"));
        assert_eq!(r.changes[0].undo_point.as_deref(), Some("undo-1"));
        assert_eq!(
            r.redactions,
            vec![Redaction {
                kind: "aws-key".into(),
                count: 2
            }]
        );
        assert_eq!(r.circuit_breaker, vec!["5 denials"]);
        assert_eq!(r.end_reason.as_deref(), Some("stopped"));
        let text = r.to_text();
        assert!(text.contains("deadbeef") && text.contains("by alice via desktop"));
        let j: serde_json::Value = serde_json::from_str(&r.to_json().unwrap()).unwrap();
        assert_eq!(j["run_id"], "run-1");
        assert_eq!(r, run_report(&c, "run-1").unwrap());
        assert!(run_report(&c, "run-").is_err(), "ambiguous prefix");
        assert!(run_report(&c, "nope").is_err());
        let runs = list_runs(&c, 10).unwrap();
        assert_eq!(runs[0].run_id, "run-2");
        assert_eq!(runs[1].calls, 3);
    }

    fn res() -> ExecResult {
        ExecResult {
            stdout: "hello\nworld\n".into(),
            stderr: "warn".into(),
            exit_code: Some(1),
            truncated: false,
            timed_out: false,
            duration_ms: 5,
        }
    }

    #[test]
    fn record_output_writes_cast_and_honours_switch() {
        let d = tempfile::tempdir().unwrap();
        let mut c = cfg(d.path());
        c.ai.record_output = false;
        record_output(&c, "run-1", "c1", "echo hi", &res()).unwrap();
        assert!(!cast_path(&c, "run-1", "c1").exists());
        c.ai.record_output = true;
        record_output(&c, "run-1", "c1", "echo hi", &res()).unwrap();
        let text = std::fs::read_to_string(cast_path(&c, "run-1", "c1")).unwrap();
        assert!(text.starts_with("{\"version\":2"));
        assert!(text.contains("hello\\r\\nworld") && text.contains("[exit 1]"));
        assert_eq!(run_casts(&c, "run-1").len(), 1);
        assert!(run_casts(&c, "run-2").is_empty());
    }

    #[test]
    fn old_casts_are_pruned() {
        let d = tempfile::tempdir().unwrap();
        let mut c = cfg(d.path());
        c.recording.retention_days = 1;
        let dir = recordings_dir(&c);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("ai-old-c.cast");
        let keep = dir.join("other.cast");
        std::fs::write(&old, "x").unwrap();
        std::fs::write(&keep, "x").unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
        for p in [&old, &keep] {
            std::fs::File::options()
                .write(true)
                .open(p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        record_output(&c, "r", "c", "true", &res()).unwrap();
        assert!(!old.exists());
        assert!(keep.exists(), "only ai-*.cast files are pruned");
    }

    #[test]
    fn render_runs_plain_and_color() {
        let now = DateTime::parse_from_rfc3339("2026-10-24T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let s_plain = Style::plain();
        assert_eq!(
            render_runs(&s_plain, &[], now),
            "No agent runs recorded yet.\n"
        );

        let runs = vec![
            RunSummary {
                run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
                client: Some("opencode 2.1.2".into()),
                policy: Some("default".into()),
                started: Some("2026-10-24T17:23:00Z".into()),
                ended: Some("2026-10-24T17:25:00Z".into()),
                calls: 38,
                denials: 1,
                changes: 1,
                end_reason: Some("client left".into()),
            },
            RunSummary {
                run_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
                client: Some("cursor".into()),
                policy: Some("strict".into()),
                started: Some("2026-10-24T17:50:00Z".into()),
                ended: None,
                calls: 4,
                denials: 0,
                changes: 0,
                end_reason: None,
            },
        ];

        let out = render_runs(&s_plain, &runs, now);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("RUN        AGENT"));
        assert!(lines[0].contains("STATE"));
        let start1 = ui::day_clock("2026-10-24T17:23:00Z", now);
        let end1 = ui::clock_hm("2026-10-24T17:25:00Z");
        let start2 = ui::day_clock("2026-10-24T17:50:00Z", now);
        let exp1 = format!("dddbee19   opencode 2.1.2   {start1}      38        1         1   ended {end1}, client left");
        assert!(
            lines[1].contains(&exp1),
            "\nACTUAL:   {:?}\nEXPECTED: {:?}",
            lines[1],
            exp1
        );
        let exp2 =
            format!("a1b2c3d4   cursor           {start2}       4        0         0   ● running");
        assert!(
            lines[2].contains(&exp2),
            "\nACTUAL:   {:?}\nEXPECTED: {:?}",
            lines[2],
            exp2
        );

        let s_color = Style {
            color: true,
            unicode: true,
        };
        let out_color = render_runs(&s_color, &runs, now);
        assert!(out_color.contains("\x1b[31m1\x1b[0m"));
        assert!(out_color.contains("\x1b[34m●\x1b[0m running"));
    }

    #[test]
    fn render_report_plain_matches_brief_structure() {
        let now = DateTime::parse_from_rfc3339("2026-10-24T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let s = Style::plain();

        let r = RunReport {
            run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
            client: Some("opencode 2.1.2".into()),
            on_behalf_of: Some("ana".into()),
            policy: Some("default".into()),
            policy_hash: Some("abc".into()),
            started: Some("2026-10-24T17:23:00Z".into()),
            ended: Some("2026-10-24T17:25:10Z".into()),
            end_reason: Some("client left".into()),
            hosts: vec!["web-1".into()],
            calls: vec![
                ReportCall {
                    seq: 1,
                    ts: "2026-10-24T17:23:58Z".into(),
                    call_id: "c1".into(),
                    tool: "run_readonly".into(),
                    host: Some("web-1".into()),
                    command: Some("touch /var/tmp/.bssh_write_test".into()),
                    path: None,
                    decision: "deny".into(),
                    reason: "writes in a read-only call".into(),
                    exit_code: None,
                    output_sha256: None,
                    redactions: vec![],
                },
                ReportCall {
                    seq: 2,
                    ts: "2026-10-24T17:24:00Z".into(),
                    call_id: "c2".into(),
                    tool: "run_readonly".into(),
                    host: Some("web-1".into()),
                    command: Some("du -xah /var/www".into()),
                    path: None,
                    decision: "allow".into(),
                    reason: "read".into(),
                    exit_code: Some(0),
                    output_sha256: None,
                    redactions: vec![],
                },
            ],
            approvals: vec![ReportApproval {
                seq: 3,
                ts: "2026-10-24T17:24:44Z".into(),
                request_id: "req1".into(),
                outcome: "approved".into(),
                by: Some("ana".into()),
                channel: Some("in-band".into()),
                reason: None,
            }],
            changes: vec![ReportChange {
                seq: 4,
                ts: "2026-10-24T17:24:47Z".into(),
                change_id: "change-696de2f2".into(),
                plan_hash: "486c2f5a".into(),
                applied: true,
                undo_point: Some("undo-point-1".into()),
                title: Some(
                    "Free 228 MB on web-1 /var/www (archive 3 old releases + stale bundle)".into(),
                ),
                approved_by: Some("ana".into()),
                approved_channel: Some("in-band".into()),
                approved_at: Some("2026-10-24T17:24:44Z".into()),
                steps_ok: Some((1, 1)),
                checks_ok: Some((2, 2)),
                undone: false,
            }],
            denials: vec!["run_readonly touch: writes in a read-only call".into()],
            redactions: vec![],
            circuit_breaker: vec![],
            recordings: vec!["ai-dddbee19-c1.cast".into()],
        };

        let text = r.render(&s, now);
        assert!(text.contains("opencode 2.1.2 · run dddbee19 · on behalf of ana · policy default"));
        let start_str = ui::day_clock("2026-10-24T17:23:00Z", now);
        let end_str = ui::clock("2026-10-24T17:25:10Z");
        assert!(text.contains(&format!(
            "{start_str} → {end_str} (2 min 10 s) · hosts web-1"
        )));
        assert!(text
            .contains("2 calls  1 allowed  1 denied  1 approved   1 change, applied, undo kept"));
        assert!(text.contains("CHANGES\n  ✓ 696de2f2  Free 228 MB on web-1 /var/www (archive 3 old releases + stale bundle)"));
        let at_str = ui::clock("2026-10-24T17:24:44Z");
        assert!(text.contains(&format!("approved by ana at {at_str} · step 1/1 ok · checks 2/2 ok · undo with bssh ai undo 696de2f2")));
        let deny_time = ui::clock("2026-10-24T17:23:58Z");
        assert!(text.contains(&format!("DENIED\n  {deny_time}  read  web-1  touch /var/tmp/.bssh_write_test · writes in a read-only call")));
        assert!(text.contains("REDACTED\n  nothing"));
        assert!(text.contains("RECORDINGS\n  1 recording · bssh ai replay dddbee19 <call>"));
        assert!(!text.contains(".cast"), "never list recording file names");
        assert!(text.contains("ALL CALLS\n  bssh ai log dddbee19"));
        assert!(
            !text.contains("exit Some"),
            "never leak Debug representations"
        );
    }
}
