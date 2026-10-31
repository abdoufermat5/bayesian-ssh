//! Tests for the approval card's layout, text, colour and full view.

use super::card::{render_card_w, render_request, Card};
use super::fixtures::{base, call, demo_plan_request, hello, now, service_request};
use super::view::View;
use crate::services::ai::types::{
    Analysis, ApprovalRequest, ApprovalSubject, RiskTag, Scope, Tier, ToolName,
};
use crate::services::ai::ui::{self, Style};

fn card(style: &Style, r: &ApprovalRequest, view: View) -> String {
    let run = hello();
    render_card_w(
        Card {
            style,
            run: &run,
            request: r,
            now: now(),
        },
        view,
        86,
    )
}

fn plain(r: &ApprovalRequest) -> String {
    card(&Style::plain(), r, View::Short)
}

fn plain_full(r: &ApprovalRequest) -> String {
    card(&Style::plain(), r, View::Full)
}

/// Colour on, to prove it paints the same layout.
fn colored_card(r: &ApprovalRequest) -> String {
    let style = Style {
        color: true,
        unicode: true,
    };
    card(&style, r, View::Short)
}

#[test]
fn a_read_outside_read_paths_says_what_and_why() {
    let mut c = call(ToolName::ReadFile, None);
    c.path = Some("/opt/app/app.py".into());
    let mut r = base(ApprovalSubject::Call {
        call: c,
        analysis: None,
    });
    r.reason = "read: /opt/app/app.py is outside read_paths".into();
    let text = plain(&r);
    assert!(text.contains("Read /opt/app/app.py on db-1"), "{text}");
    assert!(text.contains("│ read /opt/app/app.py"), "{text}");
    assert!(text.contains("reads    /opt/app/app.py"), "{text}");
    assert!(
        text.contains("why     read: /opt/app/app.py is outside read_paths"),
        "{text}"
    );
    assert!(!text.contains("not understood"), "{text}");
}

#[test]
fn plan_card_follows_the_mockup() {
    let text = plain(&demo_plan_request());
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("◆ Approval needed"));
    assert!(lines[0].ends_with("expires in 4:41"));
    assert_eq!(ui::display_width(lines[0]), 86);
    assert!(lines[1].contains("Free 228 MB on web-1"));
    assert_eq!(
        lines[2].trim(),
        "change plan 696de2f2 · 1 step · 5 checks · 1 undo · opencode 2.1.2 · run dddbee19"
    );
    assert!(text.contains("WHAT THIS DOES"));
    assert!(text.contains("read by bssh, not the agent"));
    assert!(text.contains("creates"));
    assert!(text.contains("moves"));
    assert!(text.contains("→"));
    assert!(text.contains("deletes  nothing"));
    assert!(text.contains("sudo  no"));
    assert!(text.contains("network  no"));
    assert!(text.contains("policy  default · one approver, outside the agent"));
    assert!(text.contains("STEP 1 · web-1"));
    assert!(text.contains("│ A=/var/tmp/var-www-cleanup-2026-10-24"));
    assert!(text.contains("│ install -d -m 0750"));
    assert!(text.contains("CHECKS  df -h /var/www · 4 × test"));
    assert!(text.contains("read-only"));
    assert!(text.contains("UNDO"));
    assert!(text.contains("THE AGENT SAYS"));
    assert!(text.contains("not checked by bssh"));
    assert!(text.contains("“/var/www is a 310 MB tmpfs"));
    assert!(text.contains("v for all of it"));
    let keys = lines.last().unwrap();
    assert!(keys.contains("a approve    d deny    v full text    s skip"));
    assert!(keys.ends_with("approves plan 486c2f5a only"));
    assert!(!keys.contains("g approve"));
    assert!(lines.iter().all(|l| ui::display_width(l) <= 86), "{text}");
}

#[test]
fn full_view_lifts_the_limits() {
    let r = demo_plan_request();
    let short = plain(&r);
    let full = plain_full(&r);
    assert!(full.lines().count() > short.lines().count());
    assert!(!full.contains("v for all of it"));
    assert!(full.contains("short view"));
    assert!(full.contains("│ test -f /var/tmp/a/cache/assets/bundle-old.js"));
}

#[test]
fn single_service_call_uses_the_same_frame() {
    let text = plain(&service_request(Tier::OutOfBand));
    println!("{text}");
    assert!(text
        .lines()
        .nth(1)
        .unwrap()
        .contains("Restart nginx on db-1"));
    assert!(text.contains("service request 440947ca"));
    assert!(text.contains("db-1"));
    assert!(text.contains("restarts"));
    assert!(text.contains("sudo  yes"));
    assert!(text.contains("SERVICE · db-1"));
    assert!(text.contains("│ sudo -n systemctl restart -- nginx"));
    assert!(text.contains("privilege · service"));
    assert!(!text.contains("THE AGENT SAYS"));
    let keys = text.lines().last().unwrap();
    assert!(keys.contains("g approve and allow this for 15 min"));
}

#[test]
fn two_person_calls_say_so() {
    let text = plain(&service_request(Tier::TwoPerson));
    assert!(text.contains("two different people must approve"));
}

#[test]
fn edit_call_shows_a_colored_diff() {
    let mut c = call(ToolName::EditFile, None);
    c.path = Some("/etc/nginx/nginx.conf".into());
    c.args = serde_json::json!({"patch": "-worker_processes 2;\n+worker_processes 4;"});
    let r = base(ApprovalSubject::Call {
        call: c,
        analysis: None,
    });
    let text = plain(&r);
    assert!(text.contains("Edit /etc/nginx/nginx.conf on db-1"));
    assert!(text.contains("edits    /etc/nginx/nginx.conf"));
    assert!(text.contains("-worker_processes 2;"));
    assert!(text.contains("+worker_processes 4;"));
    let colored = colored_card(&r);
    assert!(colored.contains("\x1b[32m+worker_processes 4;"));
    assert!(colored.contains("\x1b[31m-worker_processes 2;"));
}

#[test]
fn access_request_card() {
    let r = base(ApprovalSubject::Access {
        scope: Scope {
            tags: vec!["prod".into()],
            hosts: vec!["db-1".into()],
        },
        reason: "debug outage".into(),
        duration: "30m".into(),
    });
    let text = plain(&r);
    println!("{text}");
    assert!(text
        .lines()
        .nth(1)
        .unwrap()
        .contains("Wider access for 30 min"));
    assert!(text.contains("access request 440947ca"));
    assert!(text.contains("hosts  db-1"));
    assert!(text.contains("tags   prod"));
    assert!(text.contains("“debug outage”"));
    let keys = text.lines().last().unwrap();
    assert!(keys.contains("s skip"));
    assert!(!keys.contains("g approve"));
}

#[test]
fn unparsed_commands_are_flagged() {
    let a = Analysis {
        commands: vec![],
        tags: [RiskTag::Unknown].into_iter().collect(),
        unknown_reason: Some("eval".into()),
    };
    let r = base(ApprovalSubject::Call {
        call: call(ToolName::Run, Some("eval $X")),
        analysis: Some(a),
    });
    let text = plain(&r);
    assert!(text.contains("part of this is not understood: eval"));
    assert!(text.contains("not understood"));
}

#[test]
fn color_output_strips_to_the_plain_output() {
    for r in [demo_plan_request(), service_request(Tier::OutOfBand)] {
        let painted = colored_card(&r);
        assert!(painted.contains('\x1b'));
        assert_eq!(ui::strip_ansi(&painted), plain(&r));
    }
}

#[test]
fn render_request_is_a_thin_wrapper() {
    let text = render_request(&hello(), &service_request(Tier::OutOfBand));
    assert!(ui::strip_ansi(&text).contains("Approval needed"));
}
