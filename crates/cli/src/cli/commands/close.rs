//! Close command implementation - manage active sessions

use crate::cli::utils::{confirm, format_elapsed, truncate_display};
use crate::config::AppConfig;
use crate::database::Database;
use anyhow::Result;
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
fn cleanup_stale_sessions(db: &Database) -> Result<()> {
    let sessions = db.get_active_sessions()?;
    let mut cleaned = 0;

    for (conn_name, pid, _) in &sessions {
        if let Some(p) = pid {
            if live_pid(*p).is_none() {
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

/// Return the PID if it is a plausible session process that is still alive.
///
/// PIDs come from the database, so guard the `u32 → i32` conversion:
/// 0 would signal our own process group, values above `i32::MAX` wrap to
/// negative (signalling a whole process group), and 1 is init.
fn live_pid(pid: u32) -> Option<Pid> {
    let raw = i32::try_from(pid).ok().filter(|&p| p > 1)?;
    let target = Pid::from_raw(raw);
    // Signal 0 performs only the existence/permission check.
    kill(target, None).ok().map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::live_pid;

    #[test]
    fn live_pid_rejects_dangerous_values() {
        assert!(live_pid(0).is_none());
        assert!(live_pid(1).is_none());
        assert!(live_pid(u32::MAX).is_none());
        assert!(live_pid(i32::MAX as u32 + 1).is_none());
        assert!(live_pid(std::process::id()).is_some());
    }
}
