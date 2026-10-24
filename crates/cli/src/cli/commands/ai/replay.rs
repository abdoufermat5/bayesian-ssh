//! `bssh ai replay`. Owner: W6.
//!
//! Plays the asciicast files `report::record_output` wrote for a run. The loop mirrors
//! `cli/commands/replay.rs` (asciicast v2 "o" events); that file looks sessions up in the
//! sessions DB, which AI recordings are not part of.

use crate::cli::parser_ai::AiArgsReplay;
use crate::config::AppConfig;
use crate::services::ai::{audit, report};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

pub async fn execute(args: AiArgsReplay, config: AppConfig) -> Result<()> {
    let entries = audit::read_entries(&config).unwrap_or_default();
    let id = report::resolve_run_id(&entries, &args.run).unwrap_or_else(|_| args.run.clone());
    let casts = match &args.call {
        Some(call) => {
            let p = report::cast_path(&config, &id, call);
            if !p.exists() {
                anyhow::bail!("no recording for call '{call}' of run '{id}'");
            }
            vec![p]
        }
        None => report::run_casts(&config, &id),
    };
    if casts.is_empty() {
        anyhow::bail!("no recordings for run '{id}' (is ai.record_output on?)");
    }
    for p in casts {
        println!(
            "--- {}",
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("recording")
        );
        play(&p, 1.0, 2.0).await?;
    }
    Ok(())
}

async fn play(path: &Path, speed: f64, idle_cap: f64) -> Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open recording '{}'", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header: serde_json::Value = serde_json::from_str(
        &lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("recording '{}' is empty", path.display()))??,
    )
    .with_context(|| format!("invalid asciicast header in '{}'", path.display()))?;
    if header.get("version").and_then(|v| v.as_u64()) != Some(2) {
        anyhow::bail!("unsupported asciicast version in '{}'", path.display());
    }
    let mut prev = 0.0_f64;
    let mut out = std::io::stdout();
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (time, kind, data): (f64, String, String) = serde_json::from_str(&line)
            .with_context(|| format!("invalid asciicast event in '{}'", path.display()))?;
        if kind != "o" {
            continue;
        }
        let delay = ((time - prev).max(0.0) / speed).min(idle_cap);
        prev = time.max(prev);
        if delay > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        out.write_all(data.as_bytes())?;
        out.flush()?;
    }
    Ok(())
}
