use super::get_db_and_config;
use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Connection;
use std::path::PathBuf;

#[tauri::command]
pub fn import_ssh_config(file: Option<String>) -> Result<usize, String> {
    let (db, config) = get_db_and_config()?;
    let ssh_config_path = if let Some(file) = file.filter(|f| !f.trim().is_empty()) {
        PathBuf::from(file)
    } else {
        config
            .ssh_config_path
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".ssh/config")))
            .ok_or_else(|| "No SSH config path configured.".to_string())?
    };

    if !ssh_config_path.exists() {
        return Err(format!(
            "SSH config file not found: {}",
            ssh_config_path.display()
        ));
    }

    let content = std::fs::read_to_string(&ssh_config_path).map_err(|e| e.to_string())?;

    let mut imported_count = 0usize;
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port: Option<u16> = None;
    let mut current_identity_file: Option<String> = None;

    let flush_host = |host: String,
                      hostname: Option<String>,
                      user: Option<String>,
                      port: Option<u16>,
                      identity: Option<String>,
                      db: &Database,
                      cfg: &AppConfig|
     -> Result<bool, String> {
        if host.contains('*') || host.contains('?') {
            return Ok(false);
        }

        if db
            .get_connection(&host)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Ok(false);
        }

        let actual_host = hostname.unwrap_or_else(|| host.clone());
        let mut connection = Connection::new(
            host,
            actual_host,
            user.unwrap_or_else(|| cfg.default_user.clone()),
            port.unwrap_or(cfg.default_port),
            None,
            None,
            false,
            identity,
        );
        connection.add_tag("imported".to_string());
        db.add_connection(&connection).map_err(|e| e.to_string())?;
        Ok(true)
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(host) = line.strip_prefix("Host ") {
            if let Some(prev) = current_host.take() {
                if flush_host(
                    prev,
                    current_hostname.take(),
                    current_user.take(),
                    current_port.take(),
                    current_identity_file.take(),
                    &db,
                    &config,
                )? {
                    imported_count += 1;
                }
            }
            current_host = Some(host.trim().to_string());
            current_hostname = None;
            current_user = None;
            current_port = None;
            current_identity_file = None;
        } else if let Some(user) = line.strip_prefix("User ") {
            current_user = Some(user.trim().to_string());
        } else if let Some(port) = line.strip_prefix("Port ") {
            if let Ok(port) = port.trim().parse::<u16>() {
                current_port = Some(port);
            }
        } else if let Some(hostname) = line.strip_prefix("HostName ") {
            current_hostname = Some(hostname.trim().to_string());
        } else if let Some(identity_file) = line.strip_prefix("IdentityFile ") {
            current_identity_file = Some(identity_file.trim().to_string());
        }
    }

    if let Some(host) = current_host {
        if flush_host(
            host,
            current_hostname,
            current_user,
            current_port,
            current_identity_file,
            &db,
            &config,
        )? {
            imported_count += 1;
        }
    }

    Ok(imported_count)
}
