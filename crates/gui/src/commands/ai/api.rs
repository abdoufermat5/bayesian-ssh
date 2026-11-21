//! The `#[tauri::command]` surface of the AI view.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::services::ai::install::{Client, InstallOptions, detected_clients, run_install};
use bayesian_ssh::services::ai::policy;
use bayesian_ssh::services::ai::prune::{self, AiStorage, PruneOutcome, PrunePlan, PruneRequest};
use bayesian_ssh::services::ai::report_doc::{self, ReportFormat};
use bayesian_ssh::services::ai::types::{Policy, Profile};
use bayesian_ssh::services::ai::{audit, report};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::files::{self, ReportFiles};
use super::hub::AiHub;
use super::sink::TauriSink;
use super::types::{
    AiClientInstallResult, AiClientStatus, AiSettingsView, RunSummary, StoredEvent,
};
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

const MIN_APPROVAL_TIMEOUT_SECS: u64 = 15;
const MAX_APPROVAL_TIMEOUT_SECS: u64 = 86_400;

fn client_label(client: Client) -> &'static str {
    match client {
        Client::ClaudeCode => "Claude Code",
        Client::Codex => "Codex",
        Client::Cursor => "Cursor",
        Client::Vscode => "VS Code",
        Client::ClaudeDesktop => "Claude Desktop",
        Client::Omp => "OMP",
    }
}

fn clean_list(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|existing: &String| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn profile_defaults_map() -> BTreeMap<String, Policy> {
    let mut map = BTreeMap::new();
    map.insert("observe".into(), policy::builtin_profile(Profile::Observe));
    map.insert("operate".into(), policy::builtin_profile(Profile::Operate));
    map.insert("oncall".into(), policy::builtin_profile(Profile::Oncall));
    map
}

fn build_settings_view(config: &AppConfig) -> AiSettingsView {
    let config_path = config.config_file().display().to_string();
    let mut warnings = BTreeMap::new();
    for name in config.ai.policies.keys() {
        let w = policy::validation_warnings(config, name);
        if !w.is_empty() {
            warnings.insert(name.clone(), w);
        }
    }
    let available_sources = Database::new(config)
        .and_then(|db| db.list_sources())
        .map(|list| {
            let mut names: Vec<String> = list.into_iter().map(|s| s.source.name).collect();
            names.sort();
            names.dedup();
            names
        })
        .unwrap_or_default();

    let clients = detected_clients()
        .into_iter()
        .map(|c| AiClientStatus {
            id: c.client.name().to_string(),
            label: client_label(c.client).to_string(),
            config_path: c.config_path.display().to_string(),
            present: c.present,
            configured: c.configured,
        })
        .collect();

    AiSettingsView {
        policies: config.ai.policies.clone(),
        record_output: config.ai.record_output,
        approval_timeout_secs: config.ai.approval_timeout_secs,
        trusted_tag_sources: config.ai.trusted_tag_sources.clone(),
        config_path,
        warnings,
        profile_defaults: profile_defaults_map(),
        available_sources,
        clients,
    }
}

fn validate_policy_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("Give the policy a name (for example, default).".into());
    }
    if name.len() > 64 {
        return Err("Policy name must be 64 characters or fewer.".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("Policy name may only use letters, digits, '-', '_', and '.'.".into());
    }
    Ok(name.to_string())
}

fn sanitize_and_validate_policy(mut pol: Policy) -> Result<Policy, String> {
    pol.scope.tags = clean_list(pol.scope.tags);
    pol.scope.hosts = clean_list(pol.scope.hosts);
    if pol.scope.tags.is_empty() && pol.scope.hosts.is_empty() {
        return Err(
            "Policy scope cannot be empty: enable All hosts (*) or add at least one tag or host."
                .into(),
        );
    }

    pol.approval.in_client = clean_list(pol.approval.in_client);
    pol.approval.two_person = clean_list(pol.approval.two_person);
    pol.approval.two_person_hosts = clean_list(pol.approval.two_person_hosts);

    pol.read_paths = clean_list(pol.read_paths);
    pol.write_paths = clean_list(pol.write_paths);
    pol.deny_paths = clean_list(pol.deny_paths);
    pol.deny = clean_list(pol.deny);

    pol.budgets.run_ttl = pol.budgets.run_ttl.trim().to_string();
    match policy::parse_duration(&pol.budgets.run_ttl) {
        Some(d) if !d.is_zero() => {}
        _ => {
            return Err(format!(
                "Invalid run time limit '{}': use durations like 30m, 2h, 4h, or 1d.",
                pol.budgets.run_ttl
            ));
        }
    }

    let patterns = pol.redact.patterns.trim().to_ascii_lowercase();
    if patterns != "default" && patterns != "off" {
        return Err(format!(
            "Invalid redaction pattern mode '{}': use 'default' or 'off'.",
            pol.redact.patterns
        ));
    }
    pol.redact.patterns = patterns;
    Ok(pol)
}

fn apply_global_settings(
    config: &mut AppConfig,
    approval_timeout_secs: u64,
    record_output: bool,
    trusted_tag_sources: Vec<String>,
) -> Result<(), String> {
    if !(MIN_APPROVAL_TIMEOUT_SECS..=MAX_APPROVAL_TIMEOUT_SECS).contains(&approval_timeout_secs) {
        return Err(format!(
            "Approval timeout must be between {MIN_APPROVAL_TIMEOUT_SECS} seconds and 24 hours."
        ));
    }
    config.ai.approval_timeout_secs = approval_timeout_secs;
    config.ai.record_output = record_output;
    config.ai.trusted_tag_sources = clean_list(trusted_tag_sources);
    Ok(())
}

fn apply_policy_save(
    config: &mut AppConfig,
    name: &str,
    original_name: Option<&str>,
    policy: Policy,
) -> Result<(), String> {
    let name = validate_policy_name(name)?;
    let pol = sanitize_and_validate_policy(policy)?;
    let orig = original_name.map(str::trim).filter(|s| !s.is_empty());

    if let Some(old) = orig
        && old != name
    {
        if config.ai.policies.contains_key(&name) {
            return Err(format!("A policy named '{name}' already exists."));
        }
        config.ai.policies.remove(old);
    } else if orig.is_none() && config.ai.policies.contains_key(&name) {
        return Err(format!("A policy named '{name}' already exists."));
    }

    config.ai.policies.insert(name, pol);
    Ok(())
}

fn resolve_bssh_cli_binary() -> Result<PathBuf, String> {
    let candidates = if cfg!(windows) {
        ["bssh.exe", "bayesian-ssh.exe"]
    } else {
        ["bssh", "bayesian-ssh"]
    };

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in candidates {
                let p = dir.join(name);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }

    Err(
        "Could not locate the `bssh` CLI binary on PATH. Install the CLI or run `bssh ai install <client>` from your terminal."
            .into(),
    )
}

/// Configurable view of `config.ai`, built-in profile defaults, and detected AI clients.
#[tauri::command(async)]
pub fn ai_settings() -> Result<AiSettingsView, CommandError> {
    let config = AppConfig::load(None)?;
    Ok(build_settings_view(&config))
}

/// Update global AI settings (`approval_timeout_secs`, `record_output`, `trusted_tag_sources`).
#[tauri::command(async)]
pub fn ai_save_global_settings(
    approval_timeout_secs: u64,
    record_output: bool,
    trusted_tag_sources: Vec<String>,
) -> Result<AiSettingsView, CommandError> {
    let mut config = AppConfig::load(None)?;
    apply_global_settings(
        &mut config,
        approval_timeout_secs,
        record_output,
        trusted_tag_sources,
    )
    .map_err(CommandError::from)?;
    config.save()?;
    Ok(build_settings_view(&config))
}

/// Create or update a named AI policy in `config.json`.
#[tauri::command(async)]
pub fn ai_save_policy(
    name: String,
    original_name: Option<String>,
    policy: Policy,
) -> Result<AiSettingsView, CommandError> {
    let mut config = AppConfig::load(None)?;
    apply_policy_save(&mut config, &name, original_name.as_deref(), policy)
        .map_err(CommandError::from)?;
    config.save()?;
    Ok(build_settings_view(&config))
}

/// Delete a named AI policy from `config.json`.
#[tauri::command(async)]
pub fn ai_delete_policy(name: String) -> Result<AiSettingsView, CommandError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Policy name is required.".into());
    }
    let mut config = AppConfig::load(None)?;
    if config.ai.policies.remove(trimmed).is_none() {
        return Err(format!("Policy '{trimmed}' was not found.").into());
    }
    config.save()?;
    Ok(build_settings_view(&config))
}

/// Install or uninstall the `bssh` MCP server entry in a supported AI client.
#[tauri::command(async)]
pub fn ai_install_client(
    client: String,
    policy_name: String,
    guard: bool,
    uninstall: bool,
) -> Result<AiClientInstallResult, CommandError> {
    let client = Client::parse(&client)?;
    let config = AppConfig::load(None)?;
    let home =
        dirs::home_dir().ok_or_else(|| CommandError::from("Cannot find your home directory."))?;
    let ai_dir = bayesian_ssh::services::ai::ai_dir(&config);

    let policy_trimmed = policy_name.trim();
    let effective_policy = if policy_trimmed.is_empty() {
        "default"
    } else {
        policy_trimmed
    };

    let (profile, bssh_bin) = if uninstall {
        (Profile::Observe, Path::new("bssh").to_path_buf())
    } else {
        let resolved = policy::resolve(&config, effective_policy)
            .map_err(|e| CommandError::from(format!("Policy '{effective_policy}': {e}")))?;
        let bin = resolve_bssh_cli_binary().map_err(CommandError::from)?;
        (resolved.profile, bin)
    };

    let mut known_hosts = Vec::new();
    if guard
        && !uninstall
        && let Ok(db) = Database::new(&config)
        && let Ok(conns) = db.list_connections(None, false)
    {
        for c in conns {
            known_hosts.push(c.name);
            known_hosts.push(c.host);
        }
        known_hosts.sort();
        known_hosts.dedup();
    }

    let report = run_install(&InstallOptions {
        client,
        policy: effective_policy,
        profile,
        environment: &config.environment,
        approval_timeout_secs: config.ai.approval_timeout_secs,
        guard,
        dry_run: false,
        uninstall,
        home: &home,
        bssh_bin: &bssh_bin,
        ai_dir: &ai_dir,
        known_hosts: &known_hosts,
    })?;

    Ok(AiClientInstallResult {
        report,
        settings: build_settings_view(&config),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bayesian_ssh::services::ai::types::{Budgets, Scope};

    fn sample_policy() -> Policy {
        let mut p = policy::builtin_profile(Profile::Operate);
        p.scope = Scope {
            tags: vec!["staging".into()],
            hosts: Vec::new(),
        };
        p
    }

    #[test]
    fn global_settings_validate_timeout_bounds_and_dedup_sources() {
        let mut cfg = AppConfig::default_for_env("bssh-gui-ai-test");
        assert!(apply_global_settings(&mut cfg, 5, true, vec![]).is_err());
        assert!(apply_global_settings(&mut cfg, 90_000, true, vec![]).is_err());

        apply_global_settings(
            &mut cfg,
            600,
            false,
            vec!["  aws ".into(), "".into(), "aws".into(), "ansible".into()],
        )
        .expect("valid settings");
        assert_eq!(cfg.ai.approval_timeout_secs, 600);
        assert!(!cfg.ai.record_output);
        assert_eq!(cfg.ai.trusted_tag_sources, vec!["aws", "ansible"]);
    }

    #[test]
    fn policy_save_validates_name_scope_ttl_and_rename_collisions() {
        let mut cfg = AppConfig::default_for_env("bssh-gui-ai-test");

        // Reject bad name.
        assert!(apply_policy_save(&mut cfg, "bad name!", None, sample_policy()).is_err());

        // Reject empty scope.
        let mut empty = sample_policy();
        empty.scope = Scope::default();
        assert!(apply_policy_save(&mut cfg, "default", None, empty).is_err());

        // Reject invalid run_ttl.
        let mut bad_ttl = sample_policy();
        bad_ttl.budgets = Budgets {
            run_ttl: "notaduration".into(),
            ..Budgets::default()
        };
        assert!(apply_policy_save(&mut cfg, "default", None, bad_ttl).is_err());

        // Create succeeds and deduplicates lists.
        let mut p = sample_policy();
        p.scope.tags = vec![" staging ".into(), "staging".into(), "dev".into()];
        apply_policy_save(&mut cfg, "default", None, p).expect("create default");
        assert_eq!(
            cfg.ai.policies["default"].scope.tags,
            vec!["staging", "dev"]
        );

        // Creating a duplicate name without original_name fails.
        assert!(apply_policy_save(&mut cfg, "default", None, sample_policy()).is_err());

        // Updating in place succeeds.
        apply_policy_save(&mut cfg, "default", Some("default"), sample_policy())
            .expect("update default");

        // Create second policy and ensure rename collision is refused.
        apply_policy_save(&mut cfg, "oncall", None, sample_policy()).expect("create oncall");
        assert!(apply_policy_save(&mut cfg, "oncall", Some("default"), sample_policy()).is_err());

        // Valid rename removes the old key.
        apply_policy_save(&mut cfg, "staging-ops", Some("default"), sample_policy())
            .expect("rename default");
        assert!(!cfg.ai.policies.contains_key("default"));
        assert!(cfg.ai.policies.contains_key("staging-ops"));
    }

    #[test]
    fn profile_defaults_include_all_three_profiles() {
        let defaults = profile_defaults_map();
        assert_eq!(defaults.len(), 3);
        assert_eq!(defaults["observe"].budgets.changes_per_run, 0);
        assert_eq!(defaults["operate"].profile, Profile::Operate);
        assert_eq!(defaults["oncall"].budgets.run_ttl, "4h");
    }
}
