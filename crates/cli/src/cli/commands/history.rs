//! History command implementation - display session history with stats

use crate::cli::utils::format_elapsed;
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{SessionHistoryEntry, SessionStatus};
use crate::ui::{Glyph, Style, Table, Tone, heading, truncate};
use anyhow::Result;
use chrono::Duration;

/// Execute the history command
pub async fn execute(
    connection: Option<String>,
    limit: usize,
    days: Option<u32>,
    show_failed: bool,
    grep: Option<String>,
    config: AppConfig,
) -> Result<()> {
    let style = Style::terminal();
    let db = Database::new(&config)?;

    // Sessions whose process is gone would otherwise be listed as active.
    db.close_stale_sessions()?;

    if let Some(query) = grep {
        let hits = db.search_session_text(&query, connection.as_deref(), days, limit)?;
        if hits.is_empty() {
            println!(
                "{} No recorded session output matched {:?}.",
                style.mark(Glyph::Dot),
                query
            );
            return Ok(());
        }

        let meta = format!("{} match(es)", hits.len());
        println!("{}", heading(&style, "Session Search", Some(&meta)));

        let mut table = Table::new(&["CONNECTION", "STARTED", "SESSION", "SNIPPET"]);
        for hit in &hits {
            let short_id: String = hit.session_id.chars().take(8).collect();
            let snippet_line = hit
                .snippet
                .replace(['\r', '\n', '\t'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            table.row(vec![
                style.paint(Tone::Strong, &hit.connection_name),
                style.paint(
                    Tone::Dim,
                    &hit.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                ),
                style.paint(Tone::Dim, &short_id),
                snippet_line,
            ]);
        }
        println!("{}", table.render(&style));
        return Ok(());
    }

    // Get session history with filters
    let sessions = db.session_history(connection.as_deref(), limit, days, show_failed)?;

    if sessions.is_empty() {
        println!("{} No session history found.", style.mark(Glyph::Dot));
        if connection.is_some() {
            println!(
                "  {}",
                style.paint(
                    Tone::Dim,
                    "Try without the connection filter to see all history."
                )
            );
        }
        return Ok(());
    }

    // Calculate statistics
    let stats = calculate_stats(&sessions);

    let dot = style.glyph(Glyph::Dot);
    let summary = format!(
        "{} sessions {dot} {} ok ({:.1}%) {dot} {} failed {dot} avg {}",
        stats.total,
        stats.successful,
        stats.success_rate * 100.0,
        stats.failed,
        format_elapsed(stats.avg_duration)
    );

    println!("{}", heading(&style, "Session History", Some(&summary)));

    let mut table = Table::new(&[
        "SESSION",
        "CONNECTION",
        "STARTED",
        "DURATION",
        "STATUS",
        "EXIT",
    ])
    .align_right(3)
    .align_right(5);

    for session in &sessions {
        let short_id: String = session.session_id.chars().take(8).collect();
        let session_cell = if session.recording_path.is_some() {
            format!(
                "{} {}",
                style.paint(Tone::Dim, &short_id),
                style.mark(Glyph::Running)
            )
        } else {
            style.paint(Tone::Dim, &short_id)
        };

        let status_cell = format_status(&style, &session.status);
        let duration_cell = session
            .duration
            .map(format_elapsed)
            .unwrap_or_else(|| style.paint(Tone::Info, "ongoing"));
        let exit_cell = match session.exit_code {
            Some(0) => style.paint(Tone::Ok, "0"),
            Some(c) => style.paint(Tone::Bad, &c.to_string()),
            None => style.paint(Tone::Dim, "-"),
        };

        table.row(vec![
            session_cell,
            style.paint(Tone::Strong, &session.connection_name),
            style.paint(
                Tone::Dim,
                &session.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            ),
            duration_cell,
            status_cell,
            exit_cell,
        ]);
    }

    println!("{}", table.render(&style));
    Ok(())
}

/// Statistics for session history
struct SessionStats {
    total: usize,
    successful: usize,
    failed: usize,
    success_rate: f64,
    avg_duration: Duration,
}

fn calculate_stats(sessions: &[SessionHistoryEntry]) -> SessionStats {
    let total = sessions.len();
    let successful = sessions
        .iter()
        .filter(|s| matches!(s.status, SessionStatus::Terminated) && s.exit_code == Some(0))
        .count();
    let failed = sessions
        .iter()
        .filter(|s| {
            matches!(s.status, SessionStatus::Error(_))
                || (matches!(s.status, SessionStatus::Terminated) && s.exit_code != Some(0))
        })
        .count();

    let success_rate = if total > 0 {
        successful as f64 / total as f64
    } else {
        0.0
    };

    let total_duration: i64 = sessions
        .iter()
        .filter_map(|s| s.duration)
        .map(|d| d.num_seconds())
        .sum();

    let sessions_with_duration = sessions.iter().filter(|s| s.duration.is_some()).count();
    let avg_duration = if sessions_with_duration > 0 {
        Duration::seconds(total_duration / sessions_with_duration as i64)
    } else {
        Duration::zero()
    };

    SessionStats {
        total,
        successful,
        failed,
        success_rate,
        avg_duration,
    }
}

fn format_status(style: &Style, status: &SessionStatus) -> String {
    match status {
        SessionStatus::Starting => format!(
            "{} {}",
            style.paint(Tone::Info, style.glyph(Glyph::Waiting)),
            style.paint(Tone::Info, "starting")
        ),
        SessionStatus::Active => format!(
            "{} {}",
            style.paint(Tone::Ok, style.glyph(Glyph::Running)),
            style.paint(Tone::Ok, "active")
        ),
        SessionStatus::Disconnected => format!(
            "{} {}",
            style.paint(Tone::Warn, style.glyph(Glyph::Idle)),
            style.paint(Tone::Warn, "disconnected")
        ),
        SessionStatus::Terminated => format!(
            "{} {}",
            style.mark(Glyph::Ok),
            style.paint(Tone::Ok, "terminated")
        ),
        SessionStatus::Error(e) => format!(
            "{} {}",
            style.mark(Glyph::Bad),
            style.paint(Tone::Bad, &format!("error: {}", truncate(e, 24)))
        ),
    }
}
