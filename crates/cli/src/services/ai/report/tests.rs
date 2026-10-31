use super::*;
use crate::config::AppConfig;
use crate::services::ai::audit::LocalAudit;
use crate::services::ai::plan::{ChangeRecord, PlanState, StepRecord};
use crate::services::ai::types::{
    AiEvent, ApprovalOutcome, AuditSink, ChangePlan, Decision, ExecResult, Redaction,
    ServiceAction, Step, StepAction, Tier, ToolCall, ToolName,
};
use crate::services::ai::ui::{self, Style};
use crate::services::recording::recordings_dir;
use chrono::{DateTime, Utc};

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
        AiEvent::Paused {
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

#[test]
fn change_steps_come_from_the_change_record() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    fixture(&c);
    let step = |id: &str, action: StepAction| Step {
        id: id.into(),
        host: Some("web1".into()),
        action,
    };
    let ran = |id: &str, ok: bool, out: &str| StepRecord {
        id: id.into(),
        host: Some("web1".into()),
        kind: "run".into(),
        ok,
        exit_code: Some(if ok { 0 } else { 3 }),
        output_sha256: None,
        untrusted_output: out.into(),
        error: None,
    };
    let plan = ChangePlan {
        change_id: "chg1".into(),
        run_id: "run-1".into(),
        title: "Restart nginx".into(),
        reason: "stale workers".into(),
        steps: vec![
            step(
                "step-1",
                StepAction::Service {
                    unit: "nginx".into(),
                    action: ServiceAction::Restart,
                },
            ),
            step(
                "step-2",
                StepAction::EditFile {
                    path: "/etc/nginx/nginx.conf".into(),
                    patch: String::new(),
                },
            ),
        ],
        verify: vec![step(
            "step-3",
            StepAction::Run {
                command: "systemctl is-active nginx".into(),
            },
        )],
        rollback: vec![],
    };
    // The shape `plan::persist_record` writes.
    let body = serde_json::json!({
        "change_id": "chg1",
        "run_id": "run-1",
        "plan": plan,
        "plan_hash": "deadbeef",
        "approved_by": "alice@desktop",
        "state": PlanState::Failed,
        "record": ChangeRecord {
            steps: vec![ran("step-1", false, "Job failed\n")],
            ..Default::default()
        },
    });
    let dir = crate::services::ai::ai_dir(&c)
        .join("changes")
        .join("run-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("chg1.json"), body.to_string()).unwrap();

    let ch = &run_report(&c, "run-1").unwrap().changes[0];
    assert_eq!(ch.title.as_deref(), Some("Restart nginx"));
    assert_eq!(ch.reason.as_deref(), Some("stale workers"));
    assert_eq!(ch.approved_by.as_deref(), Some("alice@desktop"));
    assert_eq!(ch.steps_ok, Some((0, 1)));
    assert_eq!(ch.checks_ok, None);
    let got: Vec<(&str, &str, Option<bool>)> = ch
        .steps
        .iter()
        .map(|s| (s.phase.as_str(), s.action.as_str(), s.ok))
        .collect();
    assert_eq!(
        got,
        vec![
            ("step", "restart nginx", Some(false)),
            ("step", "edit /etc/nginx/nginx.conf", None),
            ("verify", "systemctl is-active nginx", None),
        ]
    );
    assert_eq!(ch.steps[0].exit_code, Some(3));
    assert_eq!(ch.steps[0].output.as_deref(), Some("Job failed\n"));
    // The desktop app reads these from the JSON.
    let j = serde_json::to_value(ch).unwrap();
    assert_eq!(j["title"], "Restart nginx");
    assert_eq!(j["steps"][0]["action"], "restart nginx");
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
            reason: None,
            approved_by: Some("ana".into()),
            approved_channel: Some("in-band".into()),
            approved_at: Some("2026-10-24T17:24:44Z".into()),
            steps_ok: Some((1, 1)),
            checks_ok: Some((2, 2)),
            undone: false,
            steps: Vec::new(),
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
    assert!(
        text.contains("2 calls  1 allowed  1 approved  1 denied   1 change, applied, undo kept")
    );
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
