use super::CommandError;
use super::open_db_and_config;
use bayesian_ssh::models::{Connection, Source, SourceKind};
use bayesian_ssh::services::inventory::{InventoryProvider, ssh_config::SshConfigProvider};
use std::path::PathBuf;

#[tauri::command(async)]
pub fn import_ssh_config(file: Option<String>) -> Result<usize, CommandError> {
    let (db, config) = open_db_and_config()?;
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
        return Err(format!("SSH config file not found: {}", ssh_config_path.display()).into());
    }

    let source = Source::new(
        "ssh-config-import".to_string(),
        SourceKind::SshConfig,
        serde_json::json!({ "path": ssh_config_path.to_string_lossy() }),
    );
    let discovered = SshConfigProvider.fetch(&source)?;

    let mut imported_count = 0usize;
    for host in discovered {
        if db.connection(&host.name)?.is_some() {
            continue;
        }

        let mut connection = Connection {
            key_path: host.key_path,
            bastion: host.bastion,
            bastion_user: host.bastion_user,
            ..Connection::new(
                host.name,
                host.host,
                host.user.unwrap_or_else(|| config.default_user.clone()),
                host.port.unwrap_or(config.default_port),
            )
        };
        if connection.validate().is_err() {
            continue;
        }
        connection.add_tag("imported".to_string());
        db.add_connection(&connection)?;
        imported_count += 1;
    }

    Ok(imported_count)
}
