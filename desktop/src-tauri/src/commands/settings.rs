use super::agent::start_agent;
use super::env::create_environment;
use super::env::set_active_env;
use super::import::import_ssh_config;
use super::{
    bayesian_config_root, sync_search_mode_to_config, DesktopSettings, OnboardingPayload,
    WorkspaceConfigUpdate, WorkspaceInfo,
};
use bayesian_ssh::config::AppConfig;
use std::path::PathBuf;

#[tauri::command]
pub fn get_workspace_info() -> Result<WorkspaceInfo, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn save_workspace_config(update: WorkspaceConfigUpdate) -> Result<(), String> {
    let mut config = AppConfig::load(None).map_err(|e| e.to_string())?;

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
    if let Some(mode) = update.search_mode {
        if mode == "bayesian" || mode == "fuzzy" {
            config.search_mode = mode;
        }
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

    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn needs_onboarding() -> Result<bool, String> {
    let settings_file = bayesian_config_root().join("desktop_settings.json");

    if !settings_file.exists() {
        return Ok(true);
    }

    let content = std::fs::read_to_string(&settings_file).map_err(|e| e.to_string())?;
    let settings: DesktopSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(!settings.onboarding_complete)
}

#[tauri::command]
pub fn complete_onboarding(payload: OnboardingPayload) -> Result<usize, String> {
    let profile = payload.profile_name.trim().to_string();
    if profile.is_empty() {
        return Err("Profile name is required.".to_string());
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

#[tauri::command]
pub fn load_desktop_settings() -> Result<DesktopSettings, String> {
    let settings_file = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("desktop_settings.json");

    if settings_file.exists() {
        let content = std::fs::read_to_string(&settings_file).map_err(|e| e.to_string())?;
        let mut settings: DesktopSettings =
            serde_json::from_str(&content).map_err(|e| e.to_string())?;

        // Apply custom agent socket if specified on load
        if let Some(ref sock) = settings.custom_agent_socket {
            if !sock.trim().is_empty() {
                std::env::set_var("SSH_AUTH_SOCK", sock);
            }
        } else {
            // Pre-fill from running environment so settings panel shows the live socket
            settings.custom_agent_socket = std::env::var("SSH_AUTH_SOCK")
                .ok()
                .filter(|s| !s.is_empty());
        }

        Ok(settings)
    } else {
        Ok(DesktopSettings {
            onboarding_complete: false,
            ..DesktopSettings::default()
        })
    }
}

#[tauri::command]
pub fn save_desktop_settings(settings: DesktopSettings) -> Result<(), String> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh");

    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let settings_file = config_dir.join("desktop_settings.json");

    let content = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_file, content).map_err(|e| e.to_string())?;

    // Keep workspace search mode aligned with desktop fuzzy preference
    if let Ok(mut config) = AppConfig::load(None) {
        config.search_mode = sync_search_mode_to_config(settings.fuzzy_search);
        let _ = config.save();
    }

    // Apply custom agent socket environment variable immediately
    if let Some(ref sock) = settings.custom_agent_socket {
        if !sock.trim().is_empty() {
            std::env::set_var("SSH_AUTH_SOCK", sock);
        } else {
            // Restore original parent agent if any, or remove
            std::env::remove_var("SSH_AUTH_SOCK");
        }
    } else {
        std::env::remove_var("SSH_AUTH_SOCK");
    }

    Ok(())
}
