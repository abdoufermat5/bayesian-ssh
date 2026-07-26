use super::get_db_and_config;
use bayesian_ssh::models::{ConnectionStats, SessionHistoryEntry};

#[tauri::command]
pub fn get_stats() -> Result<ConnectionStats, String> {
    let (db, _config) = get_db_and_config()?;

    db.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_history(limit: Option<usize>) -> Result<Vec<SessionHistoryEntry>, String> {
    let (db, config) = get_db_and_config()?;

    let effective_limit = limit.unwrap_or(config.max_history_size.max(1));
    db.get_session_history(None, effective_limit, None, false)
        .map_err(|e| e.to_string())
}
