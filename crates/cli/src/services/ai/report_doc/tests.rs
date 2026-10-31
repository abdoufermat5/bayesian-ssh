use super::text::WIDTH;
use super::*;
use crate::services::ai::report::{
    ReportApproval, ReportCall, ReportChange, ReportStep, RunReport,
};
use chrono::{DateTime, Local, TimeZone};

fn report() -> RunReport {
    RunReport {
        run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
        client: Some("opencode 2.1.2".into()),
        on_behalf_of: Some("ana".into()),
        policy: Some("default".into()),
        policy_hash: Some("abcdef0123".into()),
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
                command: Some("cat /etc/x | grep `a` <script>alert(1)</script> (y)".into()),
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
                command: Some(format!("du -xah /var/www {}", "x".repeat(150))),
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
            channel: Some("desktop".into()),
            reason: None,
        }],
        changes: vec![ReportChange {
            seq: 4,
            ts: "2026-10-24T17:24:47Z".into(),
            change_id: "change-696de2f2".into(),
            plan_hash: "486c2f5a".into(),
            applied: false,
            undo_point: None,
            title: Some("Restart nginx".into()),
            reason: Some("nginx holds a deleted log".into()),
            approved_by: Some("ana".into()),
            approved_channel: Some("desktop".into()),
            approved_at: Some("2026-10-24T17:24:44Z".into()),
            steps_ok: Some((0, 1)),
            checks_ok: None,
            undone: false,
            steps: vec![
                ReportStep {
                    phase: "step".into(),
                    id: "step-1".into(),
                    host: Some("web-1".into()),
                    action: "restart nginx".into(),
                    ok: Some(false),
                    exit_code: Some(1),
                    error: Some("unit failed".into()),
                    output: Some("Job for nginx.service failed.".into()),
                },
                ReportStep {
                    phase: "verify".into(),
                    id: "step-2".into(),
                    host: Some("web-1".into()),
                    action: "systemctl is-active nginx".into(),
                    ok: None,
                    exit_code: None,
                    error: None,
                    output: None,
                },
            ],
        }],
        denials: vec!["run_readonly cat: writes in a read-only call".into()],
        redactions: vec![],
        circuit_breaker: vec![],
        recordings: vec![],
    }
}

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap()
}

fn text(f: ReportFormat) -> String {
    String::from_utf8(render(&report(), f, now()).unwrap()).unwrap()
}

#[test]
fn text_tells_what_happened_to_each_step() {
    let t = text(ReportFormat::Text);
    assert!(t.contains("Stopped at a failed step: Restart nginx"), "{t}");
    assert!(t.contains("The agent says: nginx holds a deleted log"));
    assert!(t.contains("failed, exit 1: unit failed"));
    assert!(t.contains("not run"));
    assert!(t.contains("Job for nginx.service failed."));
    assert!(t.contains("2 calls: 1 allowed by the policy, 0 needed a person's approval, 1 denied"));
    assert!(t.lines().all(|l| l.chars().count() <= WIDTH), "{t}");
    // The 150-character command is kept whole, across lines.
    assert!(t.matches('x').count() >= 150);
    assert!(!t.contains("Some(") && !t.contains("None"));
}

#[test]
fn markdown_keeps_hostile_commands_inside_their_cell() {
    let md = text(ReportFormat::Markdown);
    let row = md
        .lines()
        .find(|l| l.contains("/etc/x"))
        .expect("denied call row");
    // Unescaped pipes are only the cell separators.
    let seps = row.replace("\\|", "").matches('|').count();
    assert_eq!(seps, 6, "{row}");
    assert!(row.contains("``cat /etc/x \\| grep `a`"), "{row}");
}

#[test]
fn html_escapes_remote_and_agent_text() {
    let h = text(ReportFormat::Html);
    assert!(!h.contains("<script>alert"));
    assert!(h.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(h.starts_with("<!doctype html>"));
}

#[test]
fn pdf_xref_points_at_every_object() {
    let pdf = render(&report(), ReportFormat::Pdf, now()).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    // Offsets are in bytes: the binary comment line must not be decoded.
    let tail_at = pdf.windows(10).rposition(|w| w == b"startxref\n").unwrap() + 10;
    let tail = std::str::from_utf8(&pdf[tail_at..]).unwrap();
    let start: usize = tail.lines().next().unwrap().parse().unwrap();
    let xref = std::str::from_utf8(&pdf[start..]).unwrap();
    assert!(xref.starts_with("xref\n"));
    let offsets: Vec<usize> = xref
        .lines()
        .skip(3)
        .take_while(|l| l.ends_with(" n "))
        .map(|l| l[..10].parse().unwrap())
        .collect();
    assert!(offsets.len() >= 7);
    let s = String::from_utf8_lossy(&pdf);
    for (i, o) in offsets.iter().enumerate() {
        let head = format!("{} 0 obj", i + 1);
        assert_eq!(&pdf[*o..o + head.len()], head.as_bytes());
    }
    // Parentheses in commands are escaped inside PDF strings.
    assert!(s.contains("\\(y\\)"));
}

#[test]
fn long_runs_spill_onto_more_pdf_pages() {
    let mut r = report();
    let c = r.calls[1].clone();
    r.calls = (0..200).map(|_| c.clone()).collect();
    let pdf = String::from_utf8_lossy(&render(&r, ReportFormat::Pdf, now()).unwrap()).into_owned();
    let pages = pdf.matches("/Type /Page ").count();
    assert!(pages > 1);
    assert!(pdf.contains(&format!("/Count {pages}")));
    assert!(pdf.contains(&format!("page {pages} of {pages}")));
}

#[test]
fn formats_parse_and_name_files() {
    assert_eq!("MD".parse::<ReportFormat>(), Ok(ReportFormat::Markdown));
    assert!("docx".parse::<ReportFormat>().is_err());
    let name = file_name(&report(), ReportFormat::Pdf);
    assert!(name.starts_with("bssh-run-dddbee19-2026102"), "{name}");
    assert!(name.ends_with(".pdf"));
}
