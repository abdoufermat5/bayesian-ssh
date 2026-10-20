use crate::cli::utils::{format_duration, resolve_connection};
use crate::config::AppConfig;
use crate::database::{Database, RankContext};
use crate::services::context::abbreviate_home;
use crate::services::SshService;
use anyhow::Result;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "explain", true, &config).await?;

    let db = Database::new(&config)?;
    let captured = RankContext::current(&db, &config);
    let rank_ctx = if config.prediction.enabled {
        captured.clone()
    } else {
        RankContext::default()
    };

    let (pred, rank, total_conns) = db.explain_connection(&connection, &rank_ctx)?;
    let b = &pred.breakdown;

    let prev_name = captured
        .prev_connection_id
        .and_then(|id| db.get_connection(&id.to_string()).ok().flatten())
        .map(|c| c.name);

    let repo_label = match &captured.git_root {
        Some(root) => format!("repo {}", abbreviate_home(root)),
        None => "repo".to_string(),
    };
    let hour_label = match captured.hour {
        Some(h) => format!("hour {h:02}:00 ±1"),
        None => "hour".to_string(),
    };
    let after_label = match &prev_name {
        Some(name) => format!("after {name}"),
        None => "after previous".to_string(),
    };

    let width = [
        "frequency prior".len(),
        "recency".len(),
        "success rate".len(),
        repo_label.chars().count(),
        hour_label.chars().count(),
        after_label.chars().count(),
        "context factor".len(),
    ]
    .into_iter()
    .max()
    .unwrap_or(20)
    .max(20);

    let sessions_word = if b.total_w == 1 {
        "session"
    } else {
        "sessions"
    };
    let recency_note = match connection.last_used {
        Some(dt) => format!("(last used {})", format_duration(dt)),
        None => "(never used)".to_string(),
    };

    let (repo_val, repo_note) = if let Some(lr) = b.repo {
        (
            format!("×{lr:.2}"),
            format!("({} of {} {sessions_word})", b.repo_hits, b.total_w),
        )
    } else if !config.prediction.enabled {
        ("—".to_string(), "(prediction disabled)".to_string())
    } else if !config.prediction.capture_cwd {
        ("—".to_string(), "(cwd capture disabled)".to_string())
    } else if captured.git_root.is_none() {
        ("—".to_string(), "(not in a git repository)".to_string())
    } else {
        (
            "—".to_string(),
            format!("({} of {} {sessions_word})", b.repo_hits, b.total_w),
        )
    };

    let (hour_val, hour_note) = if let Some(lr) = b.hour {
        (
            format!("×{lr:.2}"),
            format!("({} of {})", b.hour_hits, b.total_w),
        )
    } else if !config.prediction.enabled {
        ("—".to_string(), "(prediction disabled)".to_string())
    } else {
        (
            "—".to_string(),
            format!("({} of {})", b.hour_hits, b.total_w),
        )
    };

    let (after_val, after_note) = if let Some(lr) = b.after_prev {
        (
            format!("×{lr:.2}"),
            format!("({} of {})", b.after_prev_hits, b.total_w),
        )
    } else if !config.prediction.enabled {
        ("—".to_string(), "(prediction disabled)".to_string())
    } else if captured.prev_connection_id.is_none() {
        (
            "—".to_string(),
            "(no previous session in the last 30 min)".to_string(),
        )
    } else {
        (
            "—".to_string(),
            format!("({} of {})", b.after_prev_hits, b.total_w),
        )
    };

    println!(
        "{} — score {:.2} (rank {} of {} in the current context)",
        connection.name, pred.score, rank, total_conns
    );
    println!(
        "  {:<w$} {:<6.2} ({} {})",
        "frequency prior",
        b.prior,
        b.total_w,
        sessions_word,
        w = width
    );
    println!(
        "  {:<w$} {:<6.2} {}",
        "recency",
        b.recency,
        recency_note,
        w = width
    );
    println!("  {:<w$} {:.2}", "success rate", b.success, w = width);
    println!(
        "  {:<w$} {:<6} {}",
        repo_label,
        repo_val,
        repo_note,
        w = width
    );
    println!(
        "  {:<w$} {:<6} {}",
        hour_label,
        hour_val,
        hour_note,
        w = width
    );
    println!(
        "  {:<w$} {:<6} {}",
        after_label,
        after_val,
        after_note,
        w = width
    );
    println!(
        "  {:<w$} ×{:.2}",
        "context factor",
        b.context_factor,
        w = width
    );

    Ok(())
}
