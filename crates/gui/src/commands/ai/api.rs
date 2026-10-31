//! The `#[tauri::command]` surface of the AI view.

use std::sync::Arc;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::ai::report;
use bayesian_ssh::services::ai::report_doc::{self, ReportFormat};
use tauri::{AppHandle, Manager, State};

use super::hub::AiHub;
use super::sink::TauriSink;
use super::types::{AiSettingsView, RunSummary, StoredEvent};
use crate::commands::CommandError;

/// Starts the discovery task and manages the hub as Tauri state.
pub fn start(app: &AppHandle) {
    let hub = Arc::new(AiHub::new(Arc::new(TauriSink(app.clone()))));
    app.manage(Arc::clone(&hub));
    #[cfg(unix)]
    tauri::async_runtime::spawn(super::socket::discovery_loop(hub));
    // The run sockets are Unix sockets: nothing to discover elsewhere.
    #[cfg(not(unix))]
    let _ = hub;
}

#[tauri::command]
pub fn ai_list_runs(hub: State<'_, Arc<AiHub>>) -> Vec<RunSummary> {
    hub.list_runs()
}

#[tauri::command]
pub fn ai_run_events(hub: State<'_, Arc<AiHub>>, run_id: String) -> Vec<StoredEvent> {
    hub.run_events(&run_id)
}

#[tauri::command]
pub fn ai_decide(
    hub: State<'_, Arc<AiHub>>,
    run_id: String,
    request_id: String,
    approve: bool,
    reason: Option<String>,
    grant_minutes: Option<u32>,
) -> Result<(), CommandError> {
    hub.decide(&run_id, &request_id, approve, reason, grant_minutes)
}

#[tauri::command]
pub fn ai_stop(hub: State<'_, Arc<AiHub>>, run_id: String) -> Result<(), CommandError> {
    hub.stop(&run_id)
}

#[tauri::command]
pub fn ai_resume(hub: State<'_, Arc<AiHub>>, run_id: String) -> Result<(), CommandError> {
    hub.resume(&run_id)
}

#[tauri::command]
pub fn ai_revoke_grant(
    hub: State<'_, Arc<AiHub>>,
    run_id: String,
    grant_id: String,
) -> Result<(), CommandError> {
    hub.revoke_grant(&run_id, &grant_id)
}

/// Read-only view of `config.ai` for the settings page.
#[tauri::command(async)]
pub fn ai_settings() -> Result<AiSettingsView, CommandError> {
    let config = AppConfig::load(None)?;
    let config_path = config.config_file().display().to_string();
    Ok(AiSettingsView {
        policies: config.ai.policies,
        record_output: config.ai.record_output,
        approval_timeout_secs: config.ai.approval_timeout_secs,
        trusted_tag_sources: config.ai.trusted_tag_sources,
        config_path,
    })
}

/// Past runs from the active environment's audit log, newest first.
#[tauri::command(async)]
pub fn ai_reports(limit: Option<usize>) -> Result<Vec<report::RunSummary>, CommandError> {
    let config = AppConfig::load(None)?;
    Ok(report::list_runs(&config, limit.unwrap_or(200))?)
}

/// The report of one run, from the audit log and its change records.
#[tauri::command(async)]
pub fn ai_report(run_id: String) -> Result<report::RunReport, CommandError> {
    let config = AppConfig::load(None)?;
    Ok(report::run_report(&config, &run_id)?)
}

/// Save a run's report through a save dialog. `None` when the user cancels.
/// Sync like the other file dialogs (`commands::dialogs`): they run on the main thread.
#[tauri::command]
pub fn ai_report_export(
    window: tauri::WebviewWindow,
    run_id: String,
    format: String,
) -> Result<Option<String>, CommandError> {
    let format: ReportFormat = format.parse()?;
    let config = AppConfig::load(None)?;
    let r = report::run_report(&config, &run_id)?;
    let mut dialog = rfd::FileDialog::new()
        .set_title("Save run report")
        .set_file_name(report_doc::file_name(&r, format))
        .add_filter(format.label(), &[format.extension()])
        .set_parent(&window);
    if let Some(dir) = dirs::download_dir().or_else(dirs::home_dir) {
        dialog = dialog.set_directory(dir);
    }
    let _ = window.minimize();
    let picked = dialog.save_file();
    let _ = window.unminimize();
    let _ = window.set_focus();
    let Some(path) = picked else {
        return Ok(None);
    };
    let bytes = report_doc::render(&r, format, chrono::Local::now())?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
