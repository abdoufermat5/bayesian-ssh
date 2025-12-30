//! History command implementation - display session history with stats

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{SessionHistoryEntry, SessionStatus};
use anyhow::Result;
use chrono::Duration;

/// Execute the history command
pub async fn execute(
    connection: Option<String>,
    limit: usize,
    days: Option<u32>,
    show_failed: bool,
    config: AppConfig,
) -> Result<()> {
    let db = Database::new(&config)?;

    // Get session history with filters
    let sessions = db.get_session_history(connection.as_deref(), limit, days, show_failed)?;

    if sessions.is_empty() {
        println!("📋 No session history found.");
        if connection.is_some() {
            println!("   Try without the connection filter to see all history.");
        }
        return Ok(());
    }

    // Calculate statistics
    let stats = calculate_stats(&sessions);

    // Print header
    println!("📋 Session History\n");
    println!("{}", "─".repeat(80));

    // Print statistics summary
    println!(
        "📊 Summary: {} sessions | ✅ {} successful ({:.1}%) | ❌ {} failed | ⏱️  Avg duration: {}",
        stats.total,
        stats.successful,
        stats.success_rate * 100.0,
        stats.failed,
        format_duration_short(stats.avg_duration)
    );
    println!("{}", "─".repeat(80));
    println!();

    // Print session entries
    println!(
        "{:<20} {:<25} {:<12} {:<15} {}",
        "CONNECTION", "STARTED", "DURATION", "STATUS", "EXIT CODE"
    );
    println!("{}", "─".repeat(80));

    for session in &sessions {
        let status_str = format_status(&session.status);
        let duration_str = session
            .duration
            .map(|d| format_duration_short(d))
            .unwrap_or_else(|| "ongoing".to_string());
        let exit_str = session
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<20} {:<25} {:<12} {:<15} {}",
            truncate(&session.connection_name, 19),
            session.started_at.format("%Y-%m-%d %H:%M:%S"),
            duration_str,
            status_str,
            exit_str
        );
    }

    println!();
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

fn format_status(status: &SessionStatus) -> String {
    match status {
        SessionStatus::Starting => "⏳ starting".to_string(),
        SessionStatus::Active => "🟢 active".to_string(),
        SessionStatus::Disconnected => "🔌 disconnected".to_string(),
        SessionStatus::Terminated => "✅ terminated".to_string(),
        SessionStatus::Error(e) => format!("❌ error: {}", truncate(e, 20)),
    }
}

fn format_duration_short(duration: Duration) -> String {
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
