//! Close command implementation - manage active sessions

use crate::config::AppConfig;
use crate::database::Database;
use crate::cli::utils::confirm;
use anyhow::{bail, Result};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

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
        return cleanup_stale_sessions(&db);
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
    println!("{:<20} {:<10} {:<25} {}", "CONNECTION", "PID", "STARTED", "DURATION");
    println!("{}", "─".repeat(70));

    for (conn_name, pid, started_at) in &sessions {
        let duration = chrono::Utc::now().signed_duration_since(*started_at);
        let duration_str = format_duration(duration);
        let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());
        
        // Check if process is actually running
        let status = if let Some(p) = pid {
            if is_process_running(*p) { "🟢" } else { "⚠️ stale" }
        } else {
            "❓"
        };

        println!(
            "{:<20} {:<10} {:<25} {} {}",
            truncate(conn_name, 19),
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
            if is_process_running(p) {
                if !force && !confirm(&format!("Close session for '{}' (PID {})?", conn_name, p), true)? {
                    println!("Skipped.");
                    continue;
                }

                match kill(Pid::from_raw(p as i32), Signal::SIGTERM) {
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
                println!("⚠️  Session '{}' (PID {}) is stale, cleaning up...", conn_name, p);
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

    if !force && !confirm(&format!("Close all {} active sessions?", sessions.len()), false)? {
        println!("Cancelled.");
        return Ok(());
    }

    let mut closed = 0;
    let mut cleaned = 0;

    for (conn_name, pid, _) in &sessions {
        if let Some(p) = pid {
            if is_process_running(*p) {
                if kill(Pid::from_raw(*p as i32), Signal::SIGTERM).is_ok() {
                    closed += 1;
                }
            } else {
                cleaned += 1;
            }
        }
    }

    // Mark all as terminated in DB
    db.mark_all_sessions_terminated()?;

    println!("✅ Closed {} sessions, cleaned {} stale records", closed, cleaned);

    Ok(())
}

/// Clean up stale sessions (PIDs no longer running)
fn cleanup_stale_sessions(db: &Database) -> Result<()> {
    let sessions = db.get_active_sessions()?;
    let mut cleaned = 0;

    for (conn_name, pid, _) in &sessions {
        if let Some(p) = pid {
            if !is_process_running(*p) {
                if let Some(session_id) = db.get_session_id_by_pid(*p)? {
                    db.mark_session_terminated(&session_id, -1)?;
                    cleaned += 1;
                    println!("🧹 Cleaned stale session: {} (PID {})", conn_name, p);
                }
            }
        }
    }

    if cleaned == 0 {
        println!("✨ No stale sessions found.");
    } else {
        println!("\n✅ Cleaned {} stale session(s)", cleaned);
    }

    Ok(())
}

/// Check if a process is running
fn is_process_running(pid: u32) -> bool {
    // Try to send signal 0 (doesn't actually send a signal, just checks if process exists)
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

fn format_duration(duration: chrono::Duration) -> String {
    let secs = duration.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}
