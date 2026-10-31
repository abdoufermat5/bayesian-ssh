use super::CommandError;
use super::EnvInfo;
use bayesian_ssh::config::AppConfig;
use std::path::PathBuf;

#[tauri::command(async)]
pub fn get_active_env() -> Result<String, CommandError> {
    Ok(AppConfig::get_active_env())
}

#[tauri::command(async)]
pub fn set_active_env(name: String) -> Result<(), CommandError> {
    AppConfig::validate_env_name(&name)?;
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() && name != "default" {
        return Err(format!("Environment '{}' does not exist.", name).into());
    }

    AppConfig::set_active_env(&name).map_err(CommandError::from)
}

#[tauri::command(async)]
pub fn list_environments() -> Result<Vec<EnvInfo>, CommandError> {
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

#[tauri::command(async)]
pub fn create_environment(name: String) -> Result<(), CommandError> {
    AppConfig::validate_env_name(&name)?;
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if env_dir.exists() {
        return Err(format!("Environment '{}' already exists.", name).into());
    }

    std::fs::create_dir_all(&env_dir)?;

    let config = AppConfig::default_for_env(&name);
    config.save()?;

    Ok(())
}

#[tauri::command(async)]
pub fn remove_environment(name: String) -> Result<(), CommandError> {
    // Must run before `name` is joined onto the environments dir: a name like
    // "../.." would otherwise make remove_dir_all delete outside of it.
    AppConfig::validate_env_name(&name)?;
    let active_env = AppConfig::get_active_env();
    if name == active_env {
        return Err("Cannot remove the currently active environment.".into());
    }
    if name == "default" {
        return Err("Cannot remove the built-in 'default' environment.".into());
    }

    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");

    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() {
        return Err(format!("Environment '{}' does not exist.", name).into());
    }

    std::fs::remove_dir_all(&env_dir)?;
    Ok(())
}
