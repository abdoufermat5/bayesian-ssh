//! Plain-text rendering of a run report.

use crate::services::ai::ui::{self, Glyph, Style, Tone};
use chrono::{DateTime, Utc};

use super::model::{ReportCall, RunReport};

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
    // Every call is allowed, asked or denied, so the counts add up to the total.
    let asked_count = report.calls.iter().filter(|c| c.decision == "ask").count();
    let approved_str = if asked_count > 0 {
        style.paint(
            Tone::Warn,
            &format!("{asked_count} asked, {approved_count} approved"),
        )
    } else if approved_count > 0 {
        style.paint(Tone::Ok, &format!("{approved_count} approved"))
    } else {
        "0 asked".to_string()
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
            ", stopped at a failed step"
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
        "{calls_str}  {allowed_str}  {approved_str}  {denied_str}   {changes_summary}"
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
                    // `ana@cli via cli` says it twice.
                    Some(ch) if ch != "in-band" && !by.ends_with(&format!("@{ch}")) => {
                        format!(" via {ch}")
                    }
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
                details.push("stopped at a failed step");
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
