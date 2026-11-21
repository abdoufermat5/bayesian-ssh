use crate::cli::utils::{format_duration, resolve_connection};
use crate::config::AppConfig;
use crate::database::{Database, RankContext};
use crate::services::SshService;
use crate::services::context::abbreviate_home;
use crate::ui::{Glyph, Style, Table, Tone, heading};
use anyhow::Result;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    let style = Style::terminal();
    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "explain", true, &config).await?;

    let db = Database::new(&config)?;
    let captured = RankContext::current(&db, &config);
    let disabled_ctx = RankContext::default();
    let rank_ctx = if config.prediction.enabled {
        &captured
    } else {
        &disabled_ctx
    };

    let (pred, rank, total_conns) = db.explain_connection(&connection, rank_ctx)?;
    let b = &pred.breakdown;

    let prev_name = captured
        .prev_connection_id
        .and_then(|id| db.connection(&id.to_string()).ok().flatten())
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

    let sessions_word = if b.total_w == 1 {
        "session"
    } else {
        "sessions"
    };
    let recency_note = match connection.last_used {
        Some(dt) => format!("last used {}", format_duration(dt)),
        None => "never used".to_string(),
    };

    let paint_lr = |lr: f64| {
        let tone = if lr >= 1.0 { Tone::Ok } else { Tone::Warn };
        style.paint(tone, &format!("×{lr:.2}"))
    };

    let (repo_val, repo_note) = if let Some(lr) = b.repo {
        (
            paint_lr(lr),
            format!("{} of {} {sessions_word}", b.repo_hits, b.total_w),
        )
    } else if !config.prediction.enabled {
        (
            style.paint(Tone::Dim, "-"),
            "prediction disabled".to_string(),
        )
    } else if !config.prediction.capture_cwd {
        (
            style.paint(Tone::Dim, "-"),
            "cwd capture disabled".to_string(),
        )
    } else if captured.git_root.is_none() {
        (
            style.paint(Tone::Dim, "-"),
            "not in a git repository".to_string(),
        )
    } else {
        (
            style.paint(Tone::Dim, "-"),
            format!("{} of {} {sessions_word}", b.repo_hits, b.total_w),
        )
    };

    let (hour_val, hour_note) = if let Some(lr) = b.hour {
        (paint_lr(lr), format!("{} of {}", b.hour_hits, b.total_w))
    } else if !config.prediction.enabled {
        (
            style.paint(Tone::Dim, "-"),
            "prediction disabled".to_string(),
        )
    } else {
        (
            style.paint(Tone::Dim, "-"),
            format!("{} of {}", b.hour_hits, b.total_w),
        )
    };

    let (after_val, after_note) = if let Some(lr) = b.after_prev {
        (
            paint_lr(lr),
            format!("{} of {}", b.after_prev_hits, b.total_w),
        )
    } else if !config.prediction.enabled {
        (
            style.paint(Tone::Dim, "-"),
            "prediction disabled".to_string(),
        )
    } else if captured.prev_connection_id.is_none() {
        (
            style.paint(Tone::Dim, "-"),
            "no previous session in the last 30 min".to_string(),
        )
    } else {
        (
            style.paint(Tone::Dim, "-"),
            format!("{} of {}", b.after_prev_hits, b.total_w),
        )
    };

    let dot = style.glyph(Glyph::Dot);
    let meta = format!(
        "score {:.2} {dot} rank {} of {}",
        pred.score, rank, total_conns
    );
    println!("{}", heading(&style, &connection.name, Some(&meta)));

    let mut table = Table::new(&["FACTOR", "VALUE", "DETAIL"]).align_right(1);
    table.row(vec![
        "frequency prior".to_string(),
        format!("{:.2}", b.prior),
        style.paint(Tone::Dim, &format!("{} {}", b.total_w, sessions_word)),
    ]);
    table.row(vec![
        "recency".to_string(),
        format!("{:.2}", b.recency),
        style.paint(Tone::Dim, &recency_note),
    ]);
    table.row(vec![
        "success rate".to_string(),
        format!("{:.2}", b.success),
        String::new(),
    ]);
    table.row(vec![
        repo_label,
        repo_val,
        style.paint(Tone::Dim, &repo_note),
    ]);
    table.row(vec![
        hour_label,
        hour_val,
        style.paint(Tone::Dim, &hour_note),
    ]);
    table.row(vec![
        after_label,
        after_val,
        style.paint(Tone::Dim, &after_note),
    ]);
    table.row(vec![
        style.paint(Tone::Strong, "context factor"),
        style.paint(Tone::Info, &format!("×{:.2}", b.context_factor)),
        String::new(),
    ]);

    println!("{}", table.render(&style).trim_end_matches('\n'));

    Ok(())
}
