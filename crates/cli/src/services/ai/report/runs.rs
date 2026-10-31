//! The run list of `bssh ai log`.

use crate::config::AppConfig;
use crate::services::ai::audit::read_entries;
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::build::{build_report, run_ids};

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
    #[serde(skip_serializing_if = "Option::is_none", default)]
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
