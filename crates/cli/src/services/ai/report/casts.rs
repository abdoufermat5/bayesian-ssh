//! Asciicast recordings of agent output, kept next to the audit log.

use crate::config::AppConfig;
use crate::services::ai::types::ExecResult;
use crate::services::recording::{recordings_dir, Recorder};
use std::path::PathBuf;

fn safe_part(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cast_name(run_id: &str, call_id: &str) -> String {
    format!("ai-{}-{}.cast", safe_part(run_id), safe_part(call_id))
}

/// Path of the recording of one call.
pub fn cast_path(config: &AppConfig, run_id: &str, call_id: &str) -> PathBuf {
    recordings_dir(config).join(cast_name(run_id, call_id))
}

/// Recordings of a run (full run id), sorted by name.
pub fn run_casts(config: &AppConfig, run_id: &str) -> Vec<PathBuf> {
    let prefix = format!("ai-{}-", safe_part(run_id));
    let mut v: Vec<PathBuf> = std::fs::read_dir(recordings_dir(config))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".cast"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

/// Drop `ai-*.cast` files older than `recording.retention_days` (0 = keep forever).
fn prune_ai_casts(config: &AppConfig) {
    let days = config.recording.retention_days;
    if days == 0 {
        return;
    }
    let Some(cutoff) = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(u64::from(days) * 86_400))
    else {
        return;
    };
    for e in std::fs::read_dir(recordings_dir(config))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("ai-") && name.ends_with(".cast")) {
            continue;
        }
        if let Ok(m) = e.metadata().and_then(|m| m.modified())
            && m < cutoff
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Store the output of one call as an asciicast recording when `ai.record_output` is on.
pub fn record_output(
    config: &AppConfig,
    run_id: &str,
    call_id: &str,
    command: &str,
    result: &ExecResult,
) -> anyhow::Result<()> {
    if !config.ai.record_output {
        return Ok(());
    }
    prune_ai_casts(config);
    let path = cast_path(config, run_id, call_id);
    let title = format!("ai {run_id} {call_id}: {command}");
    let mut rec = Recorder::create(
        &path,
        80,
        24,
        &title,
        config.recording.max_bytes_per_session.max(64 * 1024),
    )?;
    let crlf = |s: &str| s.replace("\r\n", "\n").replace('\n', "\r\n");
    rec.output(&format!("$ {}\r\n", crlf(command)));
    for stream in [&result.stdout, &result.stderr] {
        if !stream.is_empty() {
            rec.output(&crlf(stream));
            if !stream.ends_with('\n') {
                rec.output("\r\n");
            }
        }
    }
    let status = if result.timed_out {
        "[timed out]".to_string()
    } else if let Some(c) = result.exit_code {
        format!("[exit {c}]")
    } else {
        "[no exit code]".to_string()
    };
    let trunc = if result.truncated {
        " [output truncated]"
    } else {
        ""
    };
    rec.output(&format!("{status}{trunc}\r\n"));
    rec.finish();
    Ok(())
}
