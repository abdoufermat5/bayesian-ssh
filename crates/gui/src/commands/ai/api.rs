//! The `#[tauri::command]` surface of the AI view.

use std::sync::Arc;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::ai::prune::{self, AiStorage, PruneOutcome, PrunePlan, PruneRequest};
use bayesian_ssh::services::ai::report_doc::{self, ReportFormat};
use bayesian_ssh::services::ai::{audit, report};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::files::{self, ReportFiles};
use super::hub::AiHub;
use super::sink::TauriSink;
use super::types::{AiSettingsView, RunSummary, StoredEvent};
use crate::commands::CommandError;

/// Starts the discovery task and manages the hub as Tauri state.
pub fn start(app: &AppHandle) {
    let hub = Arc::new(AiHub::new(Arc::new(TauriSink(app.clone()))));
    app.manage(Arc::clone(&hub));
    app.manage(ReportFiles::default());
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

/// One row of the reports list: the CLI's run summary plus what the list
/// searches and filters on.
#[derive(serde::Serialize)]
pub struct ReportRow {
    #[serde(flatten)]
    summary: report::RunSummary,
    on_behalf_of: Option<String>,
    hosts: Vec<String>,
    approvals: usize,
}

/// Past runs from the active environment's audit log, newest first.
#[tauri::command(async)]
pub fn ai_reports(limit: Option<usize>) -> Result<Vec<ReportRow>, CommandError> {
    let config = AppConfig::load(None)?;
    let runs = report::list_runs(&config, limit.unwrap_or(200))?;
    let entries = audit::read_entries(&config)?;
    Ok(runs
        .into_iter()
        .map(|summary| {
            let full = report::build_report(&entries, &summary.run_id);
            ReportRow {
                summary,
                on_behalf_of: full.on_behalf_of,
                hosts: full.hosts,
                approvals: full.approvals.len(),
            }
        })
        .collect())
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
    saved: State<'_, ReportFiles>,
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
    // Only now may the webview ask to open or reveal it.
    saved.allow(&path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Render a run's report as HTML into a private temp file under the app cache
/// dir and open it in the system browser (where it can be printed or saved as
/// PDF). Returns the file's path.
#[tauri::command(async)]
pub fn ai_report_view_html(
    app: AppHandle,
    saved: State<'_, ReportFiles>,
    run_id: String,
) -> Result<String, CommandError> {
    let config = AppConfig::load(None)?;
    let r = report::run_report(&config, &run_id)?;
    let bytes = report_doc::render(&r, ReportFormat::Html, chrono::Local::now())?;
    let base = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache directory: {e}"))?;
    let dir = files::temp_dir(&base)?;
    let path = dir.join(format!("{}.html", files::safe_stem(&r.run_id)));
    files::write_private(&path, &bytes)?;
    saved.allow(&path)?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|e| format!("could not open the report: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Open a report file this app saved in this session with the system default
/// app. Any other path is refused.
#[tauri::command(async)]
pub fn ai_open_path(
    app: AppHandle,
    saved: State<'_, ReportFiles>,
    path: String,
) -> Result<(), CommandError> {
    let path = saved.check(&path)?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|e| format!("could not open the file: {e}"))?;
    Ok(())
}

/// Show a report file this app saved in this session in the file manager.
/// Any other path is refused.
#[tauri::command(async)]
pub fn ai_reveal_path(
    app: AppHandle,
    saved: State<'_, ReportFiles>,
    path: String,
) -> Result<(), CommandError> {
    let path = saved.check(&path)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| format!("could not show the file: {e}"))?;
    Ok(())
}

/// Size of the AI data on disk (log, recordings, backups, runs DB, archives).
#[tauri::command(async)]
pub fn ai_storage() -> Result<AiStorage, CommandError> {
    let config = AppConfig::load(None)?;
    Ok(prune::storage(&config)?)
}

/// Dry run of a prune: what `ai_prune` would remove. Reads only.
#[tauri::command(async)]
pub fn ai_prune_plan(req: PruneRequest) -> Result<PrunePlan, CommandError> {
    let config = AppConfig::load(None)?;
    Ok(prune::plan(&config, &req)?)
}

/// Delete, archive or wipe AI audit data. Re-plans under the audit lock.
#[tauri::command(async)]
pub fn ai_prune(
    saved: State<'_, ReportFiles>,
    req: PruneRequest,
) -> Result<PruneOutcome, CommandError> {
    let config = AppConfig::load(None)?;
    let outcome = prune::execute(&config, &req)?;
    // Let the dialog's "Show in folder" reveal the archive it just wrote.
    if let Some(dir) = &outcome.archive_dir {
        let _ = saved.allow(&dir.join("manifest.json"));
    }
    Ok(outcome)
}

/// Pick the folder an archive is written to. `None` when the user cancels.
/// Sync like the other file dialogs (`commands::dialogs`): they run on the main thread.
#[tauri::command]
pub fn ai_pick_archive_dir(window: tauri::WebviewWindow) -> Result<Option<String>, CommandError> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Archive folder")
        .set_parent(&window);
    if let Some(dir) = dirs::document_dir().or_else(dirs::home_dir) {
        dialog = dialog.set_directory(dir);
    }
    let _ = window.minimize();
    let folder = dialog.pick_folder();
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(folder.map(|p| p.to_string_lossy().to_string()))
}
