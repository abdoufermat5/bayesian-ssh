use super::agent::start_agent;
use super::env::create_environment;
use super::env::set_active_env;
use super::import::import_ssh_config;
use super::CommandError;
use super::{
    bayesian_config_root, sync_search_mode_to_config, DesktopSettings, OnboardingPayload,
    WorkspaceConfigUpdate, WorkspaceInfo,
};
use bayesian_ssh::config::AppConfig;
use std::path::PathBuf;

#[tauri::command(async)]
pub fn get_workspace_info() -> Result<WorkspaceInfo, CommandError> {
    let config = AppConfig::load(None)?;
    let config_root = bayesian_config_root();
    let env_dir = config_root.join("environments").join(&config.environment);

    Ok(WorkspaceInfo {
        active_env: config.environment.clone(),
        config_root: config_root.to_string_lossy().to_string(),
        env_dir: env_dir.to_string_lossy().to_string(),
        config_path: env_dir.join("config.json").to_string_lossy().to_string(),
        database_path: config.database_path.to_string_lossy().to_string(),
        ssh_config_path: config
            .ssh_config_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        default_user: config.default_user.clone(),
        default_port: config.default_port,
        search_mode: config.search_mode.clone(),
        log_level: config.log_level.clone(),
        auto_save_history: config.auto_save_history,
        max_history_size: config.max_history_size,
    })
}

#[tauri::command(async)]
pub fn save_workspace_config(update: WorkspaceConfigUpdate) -> Result<(), CommandError> {
    let mut config = AppConfig::load(None)?;

    if let Some(user) = update.default_user {
        config.default_user = user;
    }
    if let Some(port) = update.default_port {
        config.default_port = port;
    }
    if let Some(path) = update.ssh_config_path {
        config.ssh_config_path = if path.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        };
    }
    if let Some(mode) = update.search_mode
        && (mode == "bayesian" || mode == "fuzzy")
    {
        config.search_mode = mode;
    }
    if let Some(log_level) = update.log_level {
        config.log_level = log_level;
    }
    if let Some(auto_save) = update.auto_save_history {
        config.auto_save_history = auto_save;
    }
    if let Some(max_size) = update.max_history_size {
        config.max_history_size = max_size.clamp(50, 100_000);
    }

    config.save().map_err(CommandError::from)
}

#[tauri::command(async)]
pub fn needs_onboarding() -> Result<bool, CommandError> {
    let settings_file = bayesian_config_root().join("desktop_settings.json");

    if !settings_file.exists() {
        return Ok(true);
    }

    let content = std::fs::read_to_string(&settings_file)?;
    let settings: DesktopSettings = serde_json::from_str(&content)?;
    Ok(!settings.onboarding_complete)
}

#[tauri::command(async)]
pub fn complete_onboarding(payload: OnboardingPayload) -> Result<usize, CommandError> {
    let profile = payload.profile_name.trim().to_string();
    if profile.is_empty() {
        return Err("Profile name is required.".into());
    }

    if payload.create_profile && profile != "default" {
        create_environment(profile.clone())?;
    }

    set_active_env(profile)?;

    let default_user = payload.default_user.clone();
    let default_port = payload.default_port;

    save_workspace_config(WorkspaceConfigUpdate {
        default_user: Some(default_user.clone()),
        default_port: Some(default_port),
        ssh_config_path: payload.ssh_config_path.clone(),
        search_mode: Some(sync_search_mode_to_config(payload.fuzzy_search)),
        log_level: None,
        auto_save_history: None,
        max_history_size: None,
    })?;

    let mut settings = if bayesian_config_root()
        .join("desktop_settings.json")
        .exists()
    {
        load_desktop_settings()?
    } else {
        DesktopSettings::default()
    };

    settings.theme = payload.theme;
    settings.auto_start_agent = payload.auto_start_agent;
    settings.default_user = default_user;
    settings.default_port = default_port;
    settings.fuzzy_search = payload.fuzzy_search;
    settings.onboarding_complete = true;
    save_desktop_settings(settings)?;

    if payload.auto_start_agent {
        let _ = start_agent();
    }

    if payload.import_ssh_config {
        import_ssh_config(payload.ssh_config_path)
    } else {
        Ok(0)
    }
}

#[tauri::command(async)]
pub fn load_desktop_settings() -> Result<DesktopSettings, CommandError> {
    let settings_file = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("desktop_settings.json");

    let mut settings = if settings_file.exists() {
        let content = std::fs::read_to_string(&settings_file)?;
        let mut settings: DesktopSettings = serde_json::from_str(&content)?;

        // Apply custom agent socket if specified on load. The stored value is
        // often just the live socket of a previous login (it is pre-filled
        // below and saved back), so only apply it while it still accepts
        // connections — otherwise it would clobber the working socket
        // discovered at startup.
        if let Some(sock) = &settings.custom_agent_socket {
            if !sock.trim().is_empty() && bayesian_ssh::services::agent::is_valid_socket(sock) {
                crate::agent_socket::set(Some(sock.clone()));
            }
        } else {
            // Pre-fill with the live socket so the settings panel shows it.
            settings.custom_agent_socket = crate::agent_socket::current();
        }

        settings
    } else {
        DesktopSettings {
            onboarding_complete: false,
            ..DesktopSettings::default()
        }
    };

    if let Ok(config) = AppConfig::load(None) {
        settings.record_sessions = config.recording.enabled;
        settings.record_tags = config.recording.tags;
    }

    Ok(settings)
}

#[tauri::command(async)]
pub fn save_desktop_settings(settings: DesktopSettings) -> Result<(), CommandError> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh");

    std::fs::create_dir_all(&config_dir)?;
    let settings_file = config_dir.join("desktop_settings.json");

    let content = serde_json::to_string_pretty(&settings)?;
    // Atomic replace: commands run concurrently, and two overlapping in-place
    // writes could otherwise leave a torn settings file.
    bayesian_ssh::config::atomic_write(&settings_file, &content)?;

    // Keep workspace search mode and recording settings aligned with desktop preferences
    if let Ok(mut config) = AppConfig::load(None) {
        config.search_mode = sync_search_mode_to_config(settings.fuzzy_search);
        config.recording.enabled = settings.record_sessions;
        config.recording.tags = settings
            .record_tags
            .iter()
            .filter_map(|t| bayesian_ssh::models::Connection::normalize_tag(t))
            .collect();
        let _ = config.save();
    }

    // Use the custom agent socket right away.
    match settings
        .custom_agent_socket
        .as_deref()
        .filter(|sock| !sock.trim().is_empty())
    {
        Some(sock) => crate::agent_socket::set(Some(sock.to_string())),
        // Custom socket cleared: fall back to auto-discovery instead of
        // leaving new sessions without any agent.
        None => crate::agent_socket::set(bayesian_ssh::services::agent::find_ssh_agent_socket()),
    }

    Ok(())
}
