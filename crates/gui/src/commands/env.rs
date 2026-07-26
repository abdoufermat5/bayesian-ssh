use super::EnvInfo;
use bayesian_ssh::config::AppConfig;
use std::path::PathBuf;

#[tauri::command]
pub fn get_active_env() -> Result<String, String> {
    Ok(AppConfig::get_active_env())
}

#[tauri::command]
pub fn set_active_env(name: String) -> Result<(), String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() && name != "default" {
        return Err(format!("Environment '{}' does not exist.", name));
    }

    AppConfig::set_active_env(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_environments() -> Result<Vec<EnvInfo>, String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let active_env = AppConfig::get_active_env();
    let mut envs = vec![EnvInfo {
        name: "default".to_string(),
        is_active: active_env == "default",
    }];

    if envs_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&envs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let name_str = name.to_string();
                        if name_str != "default" {
                            envs.push(EnvInfo {
                                name: name_str.clone(),
                                is_active: name_str == active_env,
                            });
                        }
                    }
                }
            }
        }
    }

    envs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(envs)
}

#[tauri::command]
pub fn create_environment(name: String) -> Result<(), String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if env_dir.exists() {
        return Err(format!("Environment '{}' already exists.", name));
    }

    std::fs::create_dir_all(&env_dir).map_err(|e| e.to_string())?;

    let config = AppConfig::default_for_env(&name);
    config.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn remove_environment(name: String) -> Result<(), String> {
    let active_env = AppConfig::get_active_env();
    if name == active_env {
        return Err("Cannot remove the currently active environment.".to_string());
    }
    if name == "default" {
        return Err("Cannot remove the built-in 'default' environment.".to_string());
    }

    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() {
        return Err(format!("Environment '{}' does not exist.", name));
    }

    std::fs::remove_dir_all(&env_dir).map_err(|e| e.to_string())?;
    Ok(())
}
