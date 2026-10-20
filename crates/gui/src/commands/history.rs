use super::get_db_and_config;
use bayesian_ssh::models::{ConnectionStats, SessionHistoryEntry, SessionTextHit};

#[tauri::command(async)]
pub fn get_stats() -> Result<ConnectionStats, String> {
    let (db, _config) = get_db_and_config()?;

    db.get_stats().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn get_history(limit: Option<usize>) -> Result<Vec<SessionHistoryEntry>, String> {
    let (db, config) = get_db_and_config()?;

    // Sessions whose process is gone (app quit/crash, killed terminal) would
    // otherwise stay "Running" forever.
    db.close_stale_sessions().map_err(|e| e.to_string())?;

    let effective_limit = limit.unwrap_or(config.max_history_size.max(1));
    db.get_session_history(None, effective_limit, None, false)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn read_recording(session_id: String) -> Result<String, String> {
    let (db, config) = get_db_and_config()?;
    let (_, raw_path) = db
        .get_recording_path(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No recording found for session '{session_id}'"))?;

    let rec_dir = bayesian_ssh::services::recording::recordings_dir(&config);
    let canon_dir = std::fs::canonicalize(&rec_dir)
        .map_err(|e| format!("Recordings directory unavailable: {e}"))?;
    let canon_file =
        std::fs::canonicalize(&raw_path).map_err(|e| format!("Recording file unavailable: {e}"))?;

    if !canon_file.starts_with(&canon_dir) {
        return Err("Refusing to read recording outside recordings directory".to_string());
    }

    std::fs::read_to_string(&canon_file).map_err(|e| format!("Failed to read recording: {e}"))
}

#[tauri::command(async)]
pub fn search_session_text(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SessionTextHit>, String> {
    let (db, _config) = get_db_and_config()?;
    let effective_limit = limit.unwrap_or(50).clamp(1, 500);
    db.search_session_text(&query, None, None, effective_limit)
        .map_err(|e| e.to_string())
}
