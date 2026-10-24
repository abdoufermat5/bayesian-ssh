//! `bssh ai log`.

use crate::cli::parser_ai::AiArgsLog;
use crate::config::AppConfig;
use crate::services::ai::feed::{self, FeedContext};
use crate::services::ai::types::AiEvent;
use crate::services::ai::ui::{self, Style};
use crate::services::ai::{audit, report};
use anyhow::Result;
use chrono::{DateTime, Utc};

pub async fn execute(args: AiArgsLog, config: AppConfig) -> Result<()> {
    let Some(run) = args.run else {
        let runs = report::list_runs(&config, args.limit)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&runs)?);
        } else {
            let style = Style::terminal();
            print!("{}", report::render_runs(&style, &runs, chrono::Utc::now()));
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
            let style = Style::terminal();
            print!("{}", r.render(&style, chrono::Utc::now()));
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
        let events: Vec<(String, AiEvent)> = events
            .into_iter()
            .map(|e| (e.ts.clone(), e.event.clone()))
            .collect();
        print!(
            "{}",
            render_events(&Style::terminal(), &events, ui::width())
        );
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

/// A run's events in the `bssh ai watch` line format, each at its own time. Nothing is
/// folded and asks get their own line: this is the record.
fn render_events(style: &Style, events: &[(String, AiEvent)], width: usize) -> String {
    let mut ctx = FeedContext::with_width(width);
    ctx.show_asks = true;
    let mut out = String::new();
    let mut last = Utc::now();
    for (ts, ev) in events {
        let at = DateTime::parse_from_rfc3339(ts)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(last);
        last = at;
        if let Some(l) = feed::event_line(style, ev, &mut ctx, at) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    // Reads whose result was never recorded.
    for l in ctx.take_held(last, chrono::Duration::zero()) {
        out.push_str(&l.render(style, width));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{Decision, ToolCall, ToolName};

    fn call(id: &str, command: &str, decision: Decision) -> AiEvent {
        AiEvent::Call {
            call: ToolCall {
                run_id: "r1".into(),
                call_id: id.into(),
                tool: ToolName::RunReadonly,
                host: Some("web-1".into()),
                command: Some(command.into()),
                path: None,
                args: Default::default(),
            },
            decision,
        }
    }

    #[test]
    fn events_read_like_the_watch_feed() {
        let ts = "2026-10-24T17:23:42Z".to_string();
        let events = vec![
            (
                ts.clone(),
                call(
                    "c1",
                    "df -h /var/www",
                    Decision::Allow {
                        reason: "read-only".into(),
                    },
                ),
            ),
            (
                ts.clone(),
                AiEvent::Result {
                    run_id: "r1".into(),
                    call_id: "c1".into(),
                    exit_code: Some(0),
                    output_sha256: String::new(),
                    redactions: vec![],
                    duration_ms: Some(180),
                    jailed: Some(true),
                },
            ),
            (
                ts.clone(),
                call(
                    "c2",
                    "touch /var/tmp/x",
                    Decision::Deny {
                        reason: "writes in a read-only call".into(),
                    },
                ),
            ),
        ];
        let out = render_events(&Style::plain(), &events, 100);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].contains("✓ read") && lines[0].ends_with("0.2 s · jail"));
        assert!(lines[1].contains("✗ read") && lines[1].contains("denied"));
        assert!(!out.contains("Some(") && !out.contains("c1"));
    }
}
