use super::CommandError;
use super::open_db_and_config;
use bayesian_ssh::models::{ConnectionStats, SessionHistoryEntry, SessionTextHit};

#[tauri::command(async)]
pub fn get_stats() -> Result<ConnectionStats, CommandError> {
    let (db, _config) = open_db_and_config()?;

    db.stats().map_err(CommandError::from)
}

#[tauri::command(async)]
pub fn get_history(limit: Option<usize>) -> Result<Vec<SessionHistoryEntry>, CommandError> {
    let (db, config) = open_db_and_config()?;

    // Sessions whose process is gone (app quit/crash, killed terminal) would
    // otherwise stay "Running" forever.
    db.close_stale_sessions()?;

    let effective_limit = limit.unwrap_or(config.max_history_size.max(1));
    db.session_history(None, effective_limit, None, false)
        .map_err(CommandError::from)
}

/// Maximum recording file size read into memory (50 MiB).
const MAX_RECORDING_BYTES: u64 = 50 * 1024 * 1024;

#[tauri::command(async)]
pub fn read_recording(session_id: String) -> Result<String, CommandError> {
    let (db, config) = open_db_and_config()?;
    let (_, raw_path) = db
        .recording_path(&session_id)?
        .ok_or_else(|| format!("No recording found for session '{session_id}'"))?;

    let rec_dir = bayesian_ssh::services::recording::recordings_dir(&config);
    let canon_dir = std::fs::canonicalize(&rec_dir)
        .map_err(|e| CommandError::from(format!("Recordings directory unavailable: {e}")))?;
    let canon_file = std::fs::canonicalize(&raw_path)
        .map_err(|e| CommandError::from(format!("Recording file unavailable: {e}")))?;

    if !canon_file.starts_with(&canon_dir) {
        return Err("Refusing to read recording outside recordings directory".into());
    }

    let meta = std::fs::metadata(&canon_file)
        .map_err(|e| CommandError::from(format!("Failed to stat recording: {e}")))?;
    if meta.len() > MAX_RECORDING_BYTES {
        return Err(format!(
            "Recording exceeds the {} MiB size limit.",
            MAX_RECORDING_BYTES / (1024 * 1024)
        )
        .into());
    }

    use std::io::Read;
    let file = std::fs::File::open(&canon_file)
        .map_err(|e| CommandError::from(format!("Failed to read recording: {e}")))?;
    let mut buf = Vec::new();
    file.take(MAX_RECORDING_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| CommandError::from(format!("Failed to read recording: {e}")))?;
    if buf.len() as u64 > MAX_RECORDING_BYTES {
        return Err(format!(
            "Recording exceeds the {} MiB size limit.",
            MAX_RECORDING_BYTES / (1024 * 1024)
        )
        .into());
    }
    String::from_utf8(buf)
        .map_err(|e| CommandError::from(format!("Recording is not valid UTF-8: {e}")))
}

#[tauri::command(async)]
pub fn search_session_text(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SessionTextHit>, CommandError> {
    let (db, _config) = open_db_and_config()?;
    let effective_limit = limit.unwrap_or(50).clamp(1, 500);
    db.search_session_text(&query, None, None, effective_limit)
        .map_err(CommandError::from)
}
