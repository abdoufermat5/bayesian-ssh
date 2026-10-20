//! Close command implementation - manage active sessions

use crate::cli::utils::{confirm, format_elapsed, truncate_display};
use crate::config::AppConfig;
use crate::database::{live_pid, Database};
use anyhow::Result;
use nix::sys::signal::{kill, Signal};

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

    if all {
        return close_all_sessions(&db, force);
    }

    if let Some(target) = target {
        return close_session(&db, &target, force);
    }

    // No target specified - list active sessions
    list_active_sessions(&db)
}

/// List all active sessions
fn list_active_sessions(db: &Database) -> Result<()> {
    let sessions = db.get_active_sessions()?;

    if sessions.is_empty() {
        println!("📋 No active sessions.");
        return Ok(());
    }

    println!("📋 Active Sessions\n");
    println!(
        "{:<20} {:<10} {:<25} DURATION",
        "CONNECTION", "PID", "STARTED"
    );
    println!("{}", "─".repeat(70));

    for (conn_name, pid, started_at) in &sessions {
        let duration = chrono::Utc::now().signed_duration_since(*started_at);
        let duration_str = format_elapsed(duration);
        let pid_str = pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());

        // Check if process is actually running
        let status = if let Some(p) = pid {
            if live_pid(*p).is_some() {
                "🟢"
            } else {
                "⚠️ stale"
            }
        } else {
            "❓"
        };

        println!(
            "{:<20} {:<10} {:<25} {} {}",
            truncate_display(conn_name, 19),
            pid_str,
            started_at.format("%Y-%m-%d %H:%M:%S"),
            duration_str,
            status
        );
    }

    println!("\n💡 Use 'bssh close <connection>' to close a session");
    println!("   Use 'bssh close --cleanup' to remove stale sessions");

    Ok(())
}

/// Close a specific session
fn close_session(db: &Database, target: &str, force: bool) -> Result<()> {
    let sessions = db.get_active_sessions_for_connection(target)?;

    if sessions.is_empty() {
        println!("❌ No active sessions found for '{}'", target);
        return Ok(());
    }

    for (session_id, conn_name, pid, _) in sessions {
        if let Some(p) = pid {
            if let Some(target) = live_pid(p) {
                if !force
                    && !confirm(
                        &format!("Close session for '{}' (PID {})?", conn_name, p),
                        true,
                    )?
                {
                    println!("Skipped.");
                    continue;
                }

                match kill(target, Signal::SIGTERM) {
                    Ok(_) => {
                        println!("✅ Sent SIGTERM to session '{}' (PID {})", conn_name, p);
                        db.mark_session_terminated(&session_id, -15)?; // SIGTERM = 15
                    }
                    Err(e) => {
                        println!("❌ Failed to kill PID {}: {}", p, e);
                    }
                }
            } else {
                // Process not running, just update the record
                println!(
                    "⚠️  Session '{}' (PID {}) is stale, cleaning up...",
                    conn_name, p
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
fn close_all_sessions(db: &Database, force: bool) -> Result<()> {
    let sessions = db.get_active_sessions()?;

    if sessions.is_empty() {
        println!("📋 No active sessions to close.");
        return Ok(());
    }

    if !force
        && !confirm(
            &format!("Close all {} active sessions?", sessions.len()),
            false,
        )?
    {
        println!("Cancelled.");
        return Ok(());
    }

    let mut closed = 0;
    let mut cleaned = 0;

    for (_conn_name, pid, _) in &sessions {
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
        "✅ Closed {} sessions, cleaned {} stale records",
        closed, cleaned
    );

    Ok(())
}

/// Clean up stale sessions (PIDs no longer running)
fn cleanup_stale_sessions(db: &Database, config: &AppConfig) -> Result<()> {
    let cleaned = db.close_stale_sessions()?;
    for (conn_name, pid) in &cleaned {
        match pid {
            Some(p) => println!("🧹 Cleaned stale session: {} (PID {})", conn_name, p),
            None => println!("🧹 Cleaned stale session: {}", conn_name),
        }
    }

    if cleaned.is_empty() {
        println!("✨ No stale sessions found.");
    } else {
        println!("\n✅ Cleaned {} stale session(s)", cleaned.len());
    }

    if config.recording.retention_days > 0 {
        let pruned = db.prune_recordings(config.recording.retention_days)?;
        for path in &pruned {
            if let Err(e) = std::fs::remove_file(path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("Failed to delete expired recording '{path}': {e}");
                }
            }
        }
        if !pruned.is_empty() {
            println!(
                "🗑️  Pruned {} expired recording(s) (older than {} days)",
                pruned.len(),
                config.recording.retention_days
            );
        }
    }

    Ok(())
}
