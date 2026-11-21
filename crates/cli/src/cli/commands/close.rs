//! Close command implementation - manage active sessions

use crate::cli::utils::{confirm, format_elapsed};
use crate::config::AppConfig;
use crate::database::{ActiveSession, Database, live_pid};
use crate::ui::{Glyph, Style, Table, Tone, heading};
use anyhow::Result;
use nix::sys::signal::{Signal, kill};

/// Whether destructive session operations ask before acting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirmation {
    /// Prompt unless the user passed `--force`.
    Ask,
    /// `--force`: act immediately.
    Skip,
}

impl From<bool> for Confirmation {
    fn from(force: bool) -> Self {
        if force { Self::Skip } else { Self::Ask }
    }
}

/// Execute the close command
pub async fn execute(
    target: Option<String>,
    all: bool,
    cleanup: bool,
    force: bool,
    config: AppConfig,
) -> Result<()> {
    let db = Database::new(&config)?;

    if cleanup {
        return cleanup_stale_sessions(&db, &config);
    }

    let confirmation = Confirmation::from(force);

    if all {
        return close_all_sessions(&db, confirmation);
    }

    if let Some(target) = target {
        return close_session(&db, &target, confirmation);
    }

    // No target specified - list active sessions
    list_active_sessions(&db)
}

/// List all active sessions
fn list_active_sessions(db: &Database) -> Result<()> {
    let style = Style::terminal();
    let sessions = db.active_sessions()?;

    if sessions.is_empty() {
        println!("{} No active sessions.", style.mark(Glyph::Dot));
        return Ok(());
    }

    let meta = format!("{}", sessions.len());
    println!("{}", heading(&style, "Active Sessions", Some(&meta)));

    let mut table = Table::new(&["CONNECTION", "PID", "STARTED", "DURATION", "STATUS"])
        .align_right(1)
        .align_right(3);

    for ActiveSession {
        connection: conn_name,
        pid,
        started_at,
        ..
    } in &sessions
    {
        let duration = chrono::Utc::now().signed_duration_since(*started_at);
        let duration_str = format_elapsed(duration);
        let pid_str = pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());

        // Check if process is actually running
        let status = if let Some(p) = pid {
            if live_pid(*p).is_some() {
                format!(
                    "{} {}",
                    style.paint(Tone::Ok, style.glyph(Glyph::Running)),
                    style.paint(Tone::Ok, "active")
                )
            } else {
                format!(
                    "{} {}",
                    style.mark(Glyph::Ask),
                    style.paint(Tone::Warn, "stale")
                )
            }
        } else {
            format!(
                "{} {}",
                style.mark(Glyph::Idle),
                style.paint(Tone::Dim, "unknown")
            )
        };

        table.row(vec![
            style.paint(Tone::Strong, conn_name),
            style.paint(Tone::Dim, &pid_str),
            style.paint(
                Tone::Dim,
                &started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            ),
            duration_str,
            status,
        ]);
    }

    println!("{}", table.render(&style));
    println!(
        "\n{}",
        style.paint(
            Tone::Dim,
            "Use 'bssh close <connection>' to close a session, or 'bssh close --cleanup' for stale ones."
        )
    );

    Ok(())
}

/// Close a specific session
fn close_session(db: &Database, target: &str, confirmation: Confirmation) -> Result<()> {
    let style = Style::terminal();
    let sessions = db.active_sessions_for_connection(target)?;

    if sessions.is_empty() {
        println!(
            "{} No active sessions found for '{}'.",
            style.mark(Glyph::Bad),
            target
        );
        return Ok(());
    }

    for ActiveSession {
        id: session_id,
        connection: conn_name,
        pid,
        ..
    } in sessions
    {
        if let Some(p) = pid {
            if let Some(target) = live_pid(p) {
                if confirmation == Confirmation::Ask
                    && !confirm(
                        &format!("Close session for '{}' (PID {})?", conn_name, p),
                        true,
                    )?
                {
                    println!("{}", style.paint(Tone::Dim, "Skipped."));
                    continue;
                }

                match kill(target, Signal::SIGTERM) {
                    Ok(_) => {
                        println!(
                            "{} Sent SIGTERM to session '{}' (PID {}).",
                            style.mark(Glyph::Ok),
                            style.paint(Tone::Strong, &conn_name),
                            p
                        );
                        db.mark_session_terminated(&session_id, -15)?; // SIGTERM = 15
                    }
                    Err(e) => {
                        println!("{} Failed to kill PID {}: {}", style.mark(Glyph::Bad), p, e);
                    }
                }
            } else {
                // Process not running, just update the record
                println!(
                    "{} Session '{}' (PID {}) is stale, cleaning up...",
                    style.mark(Glyph::Ask),
                    style.paint(Tone::Strong, &conn_name),
                    p
                );
                db.mark_session_terminated(&session_id, -1)?;
            }
        } else {
            // No PID, just mark as terminated
            db.mark_session_terminated(&session_id, -1)?;
        }
    }

    Ok(())
}

/// Close all active sessions
fn close_all_sessions(db: &Database, confirmation: Confirmation) -> Result<()> {
    let style = Style::terminal();
    let sessions = db.active_sessions()?;

    if sessions.is_empty() {
        println!("{} No active sessions to close.", style.mark(Glyph::Dot));
        return Ok(());
    }

    if confirmation == Confirmation::Ask
        && !confirm(
            &format!("Close all {} active sessions?", sessions.len()),
            false,
        )?
    {
        println!("{}", style.paint(Tone::Dim, "Cancelled."));
        return Ok(());
    }

    let mut closed = 0;
    let mut cleaned = 0;

    for ActiveSession { pid, .. } in &sessions {
        if let Some(p) = pid {
            if let Some(target) = live_pid(*p) {
                if kill(target, Signal::SIGTERM).is_ok() {
                    closed += 1;
                }
            } else {
                cleaned += 1;
            }
        }
    }

    // Mark all as terminated in DB
    db.mark_all_sessions_terminated()?;

    println!(
        "{} Closed {} session(s), cleaned {} stale record(s).",
        style.mark(Glyph::Ok),
        closed,
        cleaned
    );

    Ok(())
}

/// Clean up stale sessions (PIDs no longer running)
fn cleanup_stale_sessions(db: &Database, config: &AppConfig) -> Result<()> {
    let style = Style::terminal();
    let cleaned = db.close_stale_sessions()?;
    for (conn_name, pid) in &cleaned {
        match pid {
            Some(p) => println!(
                "{} Cleaned stale session: {} {}",
                style.mark(Glyph::Ok),
                style.paint(Tone::Strong, conn_name),
                style.paint(Tone::Dim, &format!("(PID {p})"))
            ),
            None => println!(
                "{} Cleaned stale session: {}",
                style.mark(Glyph::Ok),
                style.paint(Tone::Strong, conn_name)
            ),
        }
    }

    if cleaned.is_empty() {
        println!("{} No stale sessions found.", style.mark(Glyph::Ok));
    } else {
        println!(
            "\n{} Cleaned {} stale session(s).",
            style.mark(Glyph::Ok),
            cleaned.len()
        );
    }

    if config.recording.retention_days > 0 {
        let pruned = db.prune_recordings(config.recording.retention_days)?;
        for path in &pruned {
            if let Err(e) = std::fs::remove_file(path)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!("Failed to delete expired recording '{path}': {e}");
            }
        }
        if !pruned.is_empty() {
            println!(
                "{} Pruned {} expired recording(s) (older than {} days).",
                style.mark(Glyph::Ok),
                pruned.len(),
                config.recording.retention_days
            );
        }
    }

    Ok(())
}
