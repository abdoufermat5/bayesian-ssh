use super::*;
use crate::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, Tier, ToolCall, ToolName,
};
use crate::services::ai::types::{
    Analysis, ChangePlan, Redaction, RiskTag, SimpleCommand, Step, StepAction,
};
use crate::services::ai::ui::{self, Style};
use chrono::{DateTime, TimeZone, Utc};

fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
    // Built from local time so the formatted clock is the same in every timezone.
    chrono::Local
        .with_ymd_and_hms(2026, 10, 24, h, m, s)
        .unwrap()
        .with_timezone(&Utc)
}

const RUN: &str = "dddbee19-f151-4ee0-8473-93924f35e1d4";

fn call(id: &str, tool: ToolName, host: Option<&str>, cmd: Option<&str>) -> ToolCall {
    ToolCall {
        run_id: RUN.into(),
        call_id: id.into(),
        tool,
        host: host.map(Into::into),
        command: cmd.map(Into::into),
        path: None,
        args: serde_json::Value::Null,
    }
}

fn allow(c: ToolCall) -> AiEvent {
    AiEvent::Call {
        call: c,
        decision: Decision::Allow {
            reason: "read-only".into(),
        },
    }
}

fn deny(c: ToolCall, why: &str) -> AiEvent {
    AiEvent::Call {
        call: c,
        decision: Decision::Deny { reason: why.into() },
    }
}

fn ctx() -> FeedContext {
    FeedContext::with_width(78)
}

fn line(ev: &AiEvent, ctx: &mut FeedContext, now: DateTime<Utc>) -> Option<String> {
    event_line(&Style::plain(), ev, ctx, now)
}

/// An allowed read and its result, as the feed sees them: the line comes with the result.
fn settle(ev: &AiEvent, c: &mut FeedContext, t: DateTime<Utc>) -> Option<String> {
    let first = line(ev, c, t);
    let AiEvent::Call { call, .. } = ev else {
        return first;
    };
    if first.is_some() {
        return first;
    }
    let res = AiEvent::Result {
        run_id: call.run_id.clone(),
        call_id: call.call_id.clone(),
        exit_code: Some(0),
        output_sha256: String::new(),
        redactions: Vec::new(),
        duration_ms: Some(180),
        jailed: Some(true),
    };
    line(&res, c, t)
}

#[test]
fn allowed_read_waits_for_its_result_and_shows_its_timing() {
    let mut c = ctx();
    let ev = allow(call(
        "c1",
        ToolName::RunReadonly,
        Some("web-1"),
        Some("df -h /var/www"),
    ));
    assert_eq!(line(&ev, &mut c, at(17, 23, 42)), None);
    let res = AiEvent::Result {
        duration_ms: Some(180),
        jailed: Some(true),
        run_id: RUN.into(),
        call_id: "c1".into(),
        exit_code: Some(0),
        output_sha256: String::new(),
        redactions: vec![],
    };
    let l = line(&res, &mut c, at(17, 23, 43)).unwrap();
    assert!(
        l.starts_with("17:23:42  ✓ read          web-1   df -h /var/www")
            && l.ends_with("0.2 s · jail"),
        "{l}"
    );
    assert!(c.last_was_read());
}

#[test]
fn failed_result_names_its_call() {
    let mut c = ctx();
    line(
        &allow(call(
            "c1",
            ToolName::RunReadonly,
            Some("web-1"),
            Some("ls /nope"),
        )),
        &mut c,
        at(17, 0, 0),
    );
    let res = AiEvent::Result {
        duration_ms: None,
        jailed: None,
        run_id: RUN.into(),
        call_id: "c1".into(),
        exit_code: Some(2),
        output_sha256: String::new(),
        redactions: vec![Redaction {
            kind: "x".into(),
            count: 1,
        }],
    };
    let l = line(&res, &mut c, at(17, 0, 1)).unwrap();
    assert!(
        l.contains("✗ read") && l.contains("web-1") && l.ends_with("exit 2 · 1 redacted"),
        "{l}"
    );
    assert_eq!(l.chars().count(), 78);
}

#[test]
fn denial_says_why_on_the_right() {
    let mut c = FeedContext::with_width(100);
    let ev = deny(
        call(
            "c2",
            ToolName::RunReadonly,
            Some("web-1"),
            Some("touch /var/tmp/x"),
        ),
        "writes in a read-only call.",
    );
    let l = line(&ev, &mut c, at(17, 23, 58)).unwrap();
    assert_eq!(
        l,
        "17:23:58  ✗ read          web-1   touch /var/tmp/x                denied  writes in a read-only call"
    );
}

#[test]
fn list_hosts_has_no_host_column() {
    let mut c = ctx();
    let l = line(
        &allow(call("c0", ToolName::ListHosts, None, None)),
        &mut c,
        at(17, 23, 33),
    )
    .unwrap();
    assert_eq!(l, "17:23:33  ✓ list hosts    ");
}

fn plan_request() -> ApprovalRequest {
    let step = |id: &str, cmd: &str| Step {
        id: id.into(),
        host: Some("web-1".into()),
        action: StepAction::Run {
            command: cmd.into(),
        },
    };
    let analysis = Analysis {
        commands: vec![SimpleCommand {
            program: "mv".into(),
            args: vec!["/var/www/a".into(), "/var/tmp/x/".into()],
            wrappers: vec![],
            writes_to: vec![],
            reads_from: vec![],
            glob_args: vec![],
            cwd: None,
            tags: [RiskTag::Write].into_iter().collect(),
            rule: None,
        }],
        tags: [RiskTag::Write].into_iter().collect(),
        unknown_reason: None,
    };
    let plan = ChangePlan {
        change_id: "change-696de2f2".into(),
        run_id: RUN.into(),
        title: "Free 228 MB on web-1 /var/www".into(),
        reason: "full".into(),
        steps: vec![step("step-1", "mv /var/www/a /var/tmp/x/")],
        verify: vec![step("v1", "df"), step("v2", "test -f /var/tmp/x/a")],
        rollback: vec![step("r1", "mv /var/tmp/x/a /var/www/")],
    };
    ApprovalRequest {
        request_id: "440947ca-1111".into(),
        run_id: RUN.into(),
        client: "opencode 2.1.2".into(),
        tier: Tier::OutOfBand,
        subject: ApprovalSubject::Plan {
            plan,
            plan_hash: "486c2f5a99".into(),
            analyses: vec![Some(analysis)],
        },
        created_at: at(17, 24, 31).to_rfc3339(),
        expires_at: at(17, 29, 12).to_rfc3339(),
        reason: String::new(),
    }
}

#[test]
fn a_plan_card_counts_file_edits() {
    let mut req = plan_request();
    if let ApprovalSubject::Plan { plan, analyses, .. } = &mut req.subject {
        plan.steps.insert(
            0,
            Step {
                id: "step-0".into(),
                host: Some("web-1".into()),
                action: StepAction::EditFile {
                    path: "/etc/app/app.env".into(),
                    patch: "-PORT=8081\n+PORT=8080\n".into(),
                },
            },
        );
        analyses.insert(0, None);
    }
    let mut c = ctx();
    let text = request_lines(&Style::plain(), &req, &mut c, at(17, 24, 40), true).join("\n");
    assert!(text.contains("edits 1 path"), "{text}");
    assert!(text.contains("moves 1 path"), "{text}");
}

#[test]
fn plan_request_card_then_approval_and_change() {
    let mut c = ctx();
    let style = Style::plain();
    let card = request_lines(&style, &plan_request(), &mut c, at(17, 24, 31), true);
    assert_eq!(card.len(), 4, "{card:?}");
    assert!(
        card[0].starts_with("17:24:31  ◆ change plan   web-1   Free 228 MB"),
        "{}",
        card[0]
    );
    assert!(card[0].ends_with("waiting for you"));
    assert!(
        card[1].contains("moves 1 path") && card[1].contains("sudo no"),
        "{}",
        card[1]
    );
    assert!(card[2].starts_with("          1 step · 2 checks · 1 undo"));
    assert!(card[2].ends_with("expires in 4:41"), "{}", card[2]);
    assert!(card[3].contains("a approve    d deny    ⏎ open the full card"));
    let quiet = request_lines(&style, &plan_request(), &mut c, at(17, 24, 31), false);
    assert_eq!(quiet.len(), 3);

    let appr = AiEvent::Approval {
        request_id: "440947ca-1111".into(),
        run_id: RUN.into(),
        outcome: ApprovalOutcome::Approved {
            by: "ana".into(),
            channel: "cli".into(),
            grant: None,
        },
    };
    assert_eq!(
        line(&appr, &mut c, at(17, 24, 44)).unwrap(),
        "17:24:44  ✓ approved by ana                                      plan 486c2f5a"
    );
    c.width = 100;
    let chg = AiEvent::Change {
        run_id: RUN.into(),
        change_id: "change-696de2f2".into(),
        plan_hash: "486c2f5a99".into(),
        applied: true,
        undo_point: Some("u".into()),
    };
    assert_eq!(
        line(&chg, &mut c, at(17, 24, 47)).unwrap(),
        "17:24:47  ✓ applied       web-1   change 696de2f2                    undo with bssh ai undo 696de2f2"
    );
}

#[test]
fn denied_approval_and_expiry() {
    let mut c = ctx();
    let d = AiEvent::Approval {
        request_id: "abcdef012345".into(),
        run_id: RUN.into(),
        outcome: ApprovalOutcome::Denied {
            by: "ana".into(),
            reason: Some("too broad".into()),
        },
    };
    let l = line(&d, &mut c, at(1, 2, 3)).unwrap();
    assert!(
        l.contains("✗ denied by ana: too broad") && l.ends_with("request abcdef01"),
        "{l}"
    );
    let e = AiEvent::Approval {
        request_id: "abcdef012345".into(),
        run_id: RUN.into(),
        outcome: ApprovalOutcome::Expired,
    };
    assert!(line(&e, &mut c, at(1, 2, 4))
        .unwrap()
        .contains("request expired"));
}

#[test]
fn run_ended_counts_calls_denials_and_changes() {
    let mut c = FeedContext::with_width(110);
    c.note_run(RUN, "opencode");
    line(
        &allow(call("a", ToolName::RunReadonly, Some("h"), Some("ls"))),
        &mut c,
        at(1, 0, 0),
    );
    line(
        &deny(call("b", ToolName::Run, Some("h"), Some("rm x")), "no"),
        &mut c,
        at(1, 0, 1),
    );
    line(
        &AiEvent::Change {
            run_id: RUN.into(),
            change_id: "c".into(),
            plan_hash: "h".into(),
            applied: true,
            undo_point: None,
        },
        &mut c,
        at(1, 0, 2),
    );
    let l = line(
        &AiEvent::RunEnded {
            run_id: RUN.into(),
            reason: "client disconnected".into(),
        },
        &mut c,
        at(17, 25, 10),
    )
    .unwrap();
    assert_eq!(
        l,
        "17:25:10  ○ opencode disconnected · 2 calls · 1 denied · 1 change        report: bssh ai log dddbee19 --report"
    );
}

#[test]
fn header_line_has_the_run_facts_and_since_on_the_right() {
    let h = RunHeader {
        client: "opencode 2.1.2".into(),
        run_id: "dddbee19-f151".into(),
        policy: "default".into(),
        on_behalf_of: "ana".into(),
        hosts: vec!["web-1".into()],
        started_at: at(17, 23, 20).to_rfc3339(),
    };
    let l = run_header(&Style::plain(), &h, 100);
    assert!(l.starts_with(
        "● opencode 2.1.2   run dddbee19   policy default   on behalf of ana   hosts web-1"
    ));
    assert!(l.ends_with("since 17:23:20"));
    assert_eq!(l.chars().count(), 100);
}

#[test]
fn long_commands_truncate_to_the_width_and_escape_codes_are_dropped() {
    let mut c = FeedContext::with_width(60);
    let cmd = format!("echo \x1b[31m{}", "x".repeat(200));
    let l = line(
        &allow(call("c", ToolName::Run, Some("web-1"), Some(&cmd))),
        &mut c,
        at(1, 2, 3),
    )
    .unwrap();
    assert_eq!(ui::display_width(&l), 60, "{l}");
    assert!(l.ends_with('…') && !l.contains('\x1b'), "{l}");
}

#[test]
fn a_long_meta_is_cut_to_the_line() {
    let mut c = FeedContext::with_width(80);
    let ev = deny(
        call(
            "c",
            ToolName::RunReadonly,
            Some("web-1"),
            Some("touch /var/tmp/x"),
        ),
        &"this host has no read-only jail and the command is not read-only ".repeat(3),
    );
    let l = line(&ev, &mut c, at(1, 0, 0)).unwrap();
    assert!(ui::display_width(&l) <= 80 && l.ends_with('…'), "{l}");
}

#[test]
fn meta_wins_over_text_when_both_do_not_fit() {
    let mut c = FeedContext::with_width(80);
    let ev = deny(
        call("c", ToolName::Run, Some("web-1"), Some(&"y".repeat(100))),
        "writes in a read-only call",
    );
    let l = line(&ev, &mut c, at(1, 2, 3)).unwrap();
    assert!(
        l.ends_with("denied  writes in a read-only call") && ui::display_width(&l) <= 80,
        "{l}"
    );
}

#[test]
fn folder_hides_the_fourth_read_on_and_notes_how_many() {
    let style = Style::plain();
    let mut c = ctx();
    let mut f = Folder::default();
    let mut shown = Vec::new();
    for i in 0..25 {
        let ev = allow(call(
            &format!("c{i}"),
            ToolName::RunReadonly,
            Some("web-1"),
            Some("ls"),
        ));
        let l = settle(&ev, &mut c, at(1, 0, i));
        shown.extend(f.push(&style, c.last_was_read(), l));
    }
    assert_eq!(shown.len(), 3);
    let d = deny(
        call("d", ToolName::RunReadonly, Some("web-1"), Some("touch x")),
        "writes",
    );
    let l = line(&d, &mut c, at(1, 1, 0));
    let out = f.push(&style, c.last_was_read(), l);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0], "          … 22 more reads, all allowed");
    assert!(out[1].contains("✗ read"));
    // A new streak starts over.
    let ev = allow(call("e", ToolName::RunReadonly, Some("web-1"), Some("ls")));
    let l = settle(&ev, &mut c, at(1, 2, 0));
    assert_eq!(f.push(&style, c.last_was_read(), l).len(), 1);
}

#[test]
fn three_reads_do_not_fold_and_toggling_shows_everything() {
    let style = Style::plain();
    let mut c = ctx();
    let mut f = Folder::default();
    let mut n = 0;
    for i in 0..3 {
        let ev = allow(call(&format!("c{i}"), ToolName::ReadFile, Some("h"), None));
        let l = settle(&ev, &mut c, at(1, 0, i));
        n += f.push(&style, c.last_was_read(), l).len();
    }
    assert_eq!(n, 3);
    assert!(!f.toggle());
    let mut n = 0;
    for i in 0..10 {
        let ev = allow(call(&format!("d{i}"), ToolName::ReadFile, Some("h"), None));
        let l = settle(&ev, &mut c, at(1, 0, i));
        n += f.push(&style, c.last_was_read(), l).len();
    }
    assert_eq!(n, 10);
}

#[test]
fn flush_prints_the_note_once() {
    let style = Style::plain();
    let mut c = ctx();
    let mut f = Folder::default();
    for i in 0..5 {
        let ev = allow(call(&format!("c{i}"), ToolName::Observe, Some("h"), None));
        let l = settle(&ev, &mut c, at(1, 0, i));
        f.push(&style, c.last_was_read(), l);
    }
    assert_eq!(
        f.flush(&style).unwrap(),
        "          … 2 more reads, all allowed"
    );
    assert_eq!(f.flush(&style), None);
}

#[test]
fn color_marks_the_verdict_and_keeps_the_layout() {
    let color = Style {
        color: true,
        unicode: true,
    };
    let mut c = ctx();
    let ev = deny(call("c", ToolName::Run, Some("h"), Some("rm x")), "no");
    let l = event_line(&color, &ev, &mut c, at(1, 2, 3)).unwrap();
    assert!(
        l.contains("\x1b[31m✗\x1b[0m") && l.contains("\x1b[31mdenied  no\x1b[0m"),
        "{l:?}"
    );
    assert_eq!(ui::display_width(&l), 78);
}

#[test]
fn ascii_fallback_has_no_unicode_marks() {
    let ascii = Style {
        color: false,
        unicode: false,
    };
    let mut c = ctx();
    let l = event_line(
        &ascii,
        &allow(call("c", ToolName::Run, Some("h"), Some("ls"))),
        &mut c,
        at(1, 2, 3),
    )
    .unwrap();
    assert!(l.contains("+ run"), "{l}");
}
