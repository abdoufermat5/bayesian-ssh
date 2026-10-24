//! Run reports, recordings of agent output, bssh ai log / replay.
//!
//! Owner: W6. Reports are built from the audit log only, never by a model.

use super::audit::{read_entries, AuditEntry};
use super::types::{AiEvent, ApprovalOutcome, Decision, ExecResult, Redaction};
use crate::config::AppConfig;
use crate::services::recording::{recordings_dir, Recorder};
use anyhow::Result;
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
    Ok(r)
}

impl RunReport {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let dash = |o: &Option<String>| o.clone().unwrap_or_else(|| "-".into());
        let _ = writeln!(s, "Run {}", self.run_id);
        let _ = writeln!(
            s,
            "  client: {}  on behalf of: {}  policy: {} ({})",
            dash(&self.client),
            dash(&self.on_behalf_of),
            dash(&self.policy),
            dash(&self.policy_hash)
        );
        let _ = writeln!(
            s,
            "  started: {}  ended: {} ({})",
            dash(&self.started),
            dash(&self.ended),
            dash(&self.end_reason)
        );
        let hosts = if self.hosts.is_empty() {
            "-".to_string()
        } else {
            self.hosts.join(", ")
        };
        let _ = writeln!(s, "  hosts: {hosts}");
        let _ = writeln!(s, "\nCalls ({})", self.calls.len());
        for c in &self.calls {
            let what = c.command.as_deref().or(c.path.as_deref()).unwrap_or("");
            let exit = c
                .exit_code
                .map(|e| format!(" exit {e}"))
                .unwrap_or_default();
            let _ = writeln!(
                s,
                "  #{} {} {} [{}]{} {} ({})",
                c.seq,
                c.tool,
                c.host.as_deref().unwrap_or("-"),
                c.decision,
                exit,
                what,
                c.reason
            );
        }
        let _ = writeln!(s, "\nApprovals ({})", self.approvals.len());
        for a in &self.approvals {
            let _ = writeln!(
                s,
                "  #{} {} {} by {} via {}{}",
                a.seq,
                a.request_id,
                a.outcome,
                a.by.as_deref().unwrap_or("-"),
                a.channel.as_deref().unwrap_or("-"),
                a.reason
                    .as_ref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            );
        }
        let _ = writeln!(s, "\nChanges ({})", self.changes.len());
        for c in &self.changes {
            let _ = writeln!(
                s,
                "  #{} {} plan {} {} undo point: {}",
                c.seq,
                c.change_id,
                c.plan_hash,
                if c.applied { "applied" } else { "not applied" },
                c.undo_point.as_deref().unwrap_or("-")
            );
        }
        let _ = writeln!(s, "\nDenials ({})", self.denials.len());
        for d in &self.denials {
            let _ = writeln!(s, "  {d}");
        }
        let _ = writeln!(s, "\nRedactions");
        if self.redactions.is_empty() {
            let _ = writeln!(s, "  none");
        }
        for x in &self.redactions {
            let _ = writeln!(s, "  {}: {}", x.kind, x.count);
        }
        if !self.circuit_breaker.is_empty() {
            let _ = writeln!(s, "\nCircuit breaker tripped");
            for c in &self.circuit_breaker {
                let _ = writeln!(s, "  {c}");
            }
        }
        if !self.recordings.is_empty() {
            let _ = writeln!(s, "\nRecordings");
            for r in &self.recordings {
                let _ = writeln!(s, "  {r}");
            }
        }
        s
    }
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
            }
        })
        .collect();
    v.reverse();
    v.truncate(limit);
    Ok(v)
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
}
