//! Replay command implementation — stream a recorded asciicast v2 session to stdout.

use crate::config::AppConfig;
use crate::database::Database;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

pub async fn execute(session: String, speed: f64, idle_cap: f64, config: AppConfig) -> Result<()> {
    if !speed.is_finite() || speed <= 0.0 {
        anyhow::bail!("--speed must be a positive number (got {speed})");
    }
    if !idle_cap.is_finite() || idle_cap < 0.0 {
        anyhow::bail!("--idle-cap must be a non-negative number (got {idle_cap})");
    }

    let db = Database::new(&config)?;
    let (_, raw_path) = db
        .get_recording_path(&session)?
        .ok_or_else(|| anyhow::anyhow!("no recording found for session '{session}'"))?;

    let path = Path::new(&raw_path);
    if !path.exists() {
        anyhow::bail!(
            "recording file for session '{}' is missing ({})",
            session,
            path.display()
        );
    }

    let file = File::open(path)
        .with_context(|| format!("failed to open recording '{}'", path.display()))?;
    let mut lines = BufReader::new(file).lines();

    let header_line = loop {
        match lines.next() {
            Some(Ok(l)) if l.trim().is_empty() => continue,
            Some(Ok(l)) => break l,
            Some(Err(e)) => return Err(e.into()),
            None => anyhow::bail!("recording file '{}' is empty", path.display()),
        }
    };

    let header: serde_json::Value = serde_json::from_str(&header_line)
        .with_context(|| format!("invalid asciicast header in '{}'", path.display()))?;
    if header.get("version").and_then(|v| v.as_u64()) != Some(2) {
        anyhow::bail!(
            "unsupported asciicast version in '{}': expected version 2",
            path.display()
        );
    }

    let mut prev_time = 0.0_f64;
    let mut stdout = std::io::stdout();

    for line_res in lines {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }
        let (time, kind, data): (f64, String, String) = serde_json::from_str(&line)
            .with_context(|| format!("invalid asciicast event in '{}'", path.display()))?;
        if kind != "o" {
            continue;
        }
        let raw_gap = (time - prev_time).max(0.0);
        prev_time = time.max(prev_time);
        let delay = (raw_gap / speed).min(idle_cap);
        if delay > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        stdout.write_all(data.as_bytes())?;
        stdout.flush()?;
    }

    Ok(())
}
