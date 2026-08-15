use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Connection;
use bayesian_ssh::services::security;

use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn list_ssh_keys() -> Result<Vec<security::SshKeyInfo>, String> {
    security::scan_ssh_keys()
}

#[tauri::command]
pub fn generate_ssh_key(name: String, key_type: Option<String>) -> Result<security::SshKeyInfo, String> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| "Could not resolve home directory".to_string())?
        .join(".ssh");

    fs::create_dir_all(&ssh_dir).map_err(|e| e.to_string())?;
    bayesian_ssh::config::enforce_secure_dir(&ssh_dir);

    let key_path = ssh_dir.join(&name);
    if key_path.exists() {
        return Err(format!("Key file already exists at {}", key_path.display()));
    }

    let kt = key_type.unwrap_or_else(|| "ed25519".to_string());

    let status = Command::new("ssh-keygen")
        .arg("-t")
        .arg(&kt)
        .arg("-f")
        .arg(&key_path)
        .arg("-N")
        .arg("")
        .status()
        .map_err(|e| format!("Failed to run ssh-keygen: {e}"))?;

    if !status.success() {
        return Err(format!("ssh-keygen exited with status {status}"));
    }

    bayesian_ssh::config::enforce_secure_file(&key_path);
    let pub_key_path = PathBuf::from(format!("{}.pub", key_path.display()));
    if pub_key_path.exists() {
        let mut perms = fs::metadata(&pub_key_path).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o644);
        let _ = fs::set_permissions(&pub_key_path, perms);
    }

    security::scan_ssh_keys()?
        .into_iter()
        .find(|k| k.name == name)
        .ok_or_else(|| "Key generated but details could not be loaded".to_string())
}

#[tauri::command]
pub fn copy_ssh_key_to_target(target: String, key_path: Option<String>) -> Result<String, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;

    let conn = database
        .get_connection_or_alias(&target)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{target}' not found"))?;

    let pub_key_path = if let Some(k) = key_path {
        let p = PathBuf::from(&k);
        if p.extension().is_some_and(|e| e == "pub") {
            p
        } else {
            PathBuf::from(format!("{k}.pub"))
        }
    } else if let Some(ref k) = conn.key_path {
        PathBuf::from(format!("{k}.pub"))
    } else {
        let home = dirs::home_dir()
            .ok_or_else(|| "Could not resolve home directory".to_string())?;
        let default_ed25519 = home.join(".ssh/id_ed25519.pub");
        let default_rsa = home.join(".ssh/id_rsa.pub");
        if default_ed25519.exists() {
            default_ed25519
        } else if default_rsa.exists() {
            default_rsa
        } else {
            return Err("No public key found. Generate one first using the Key Manager.".to_string());
        }
    };

    if !pub_key_path.exists() {
        return Err(format!("Public key file not found: {}", pub_key_path.display()));
    }

    let pub_key_content = fs::read_to_string(&pub_key_path)
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();

    // Shell-quote the public key content to prevent command injection.
    let quoted_key = format!("'{}'", pub_key_content.replace('\'', "'\"'\"'"));
    let remote_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo {quoted_key} >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys"
    );

    let mut ssh_argv = vec!["ssh".to_string(), "-p".to_string(), conn.port.to_string()];
    if let Some(bastion) = &conn.bastion {
        let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
        ssh_argv.push("-J".to_string());
        ssh_argv.push(format!("{}@{}", bu, bastion));
    }
    ssh_argv.push(format!("{}@{}", conn.user, conn.host));
    ssh_argv.push(remote_cmd);

    let (bin, args) = ssh_argv.split_first().unwrap();
    let status = Command::new(bin)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to execute ssh: {e}"))?;

    if status.success() {
        Ok(format!(
            "Public key ({}) successfully copied to {}@{}",
            pub_key_path.display(),
            conn.user,
            conn.host
        ))
    } else {
        Err(format!("SSH failed with exit code {}", status))
    }
}

#[tauri::command]
pub fn run_security_audit() -> Result<security::AuditReportDto, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;
    let connections = database.list_connections(None, false).map_err(|e| e.to_string())?;

    Ok(security::build_audit_report(&config, &connections))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackupEnvironmentData {
    pub name: String,
    pub config: AppConfig,
    pub connections: Vec<Connection>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FullBackupPayload {
    pub version: u32,
    pub active_environment: String,
    pub global_config: AppConfig,
    pub environments: Vec<BackupEnvironmentData>,
}

#[tauri::command]
pub fn export_connections_payload(
    output_path: Option<String>,
    passphrase: Option<String>,
    _format: Option<String>,
    tag: Option<String>,
) -> Result<String, String> {
    let global_config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let active_env = AppConfig::get_active_env();
    let env_list = crate::commands::env::list_environments().unwrap_or_default();

    let mut backup_environments = Vec::new();
    let mut total_connections_count = 0;

    for env_info in env_list {
        let env_cfg = match AppConfig::load(Some(env_info.name.clone())) {
            Ok(cfg) => cfg,
            Err(_) => AppConfig::default_for_env(&env_info.name),
        };

        let mut env_connections = Vec::new();
        if let Ok(db) = Database::new(&env_cfg) {
            if let Ok(conns) = db.list_connections(tag.as_deref(), false) {
                total_connections_count += conns.len();
                env_connections = conns;
            }
        }

        backup_environments.push(BackupEnvironmentData {
            name: env_info.name,
            config: env_cfg,
            connections: env_connections,
        });
    }

    let payload = FullBackupPayload {
        version: 2,
        active_environment: active_env,
        global_config,
        environments: backup_environments,
    };

    let raw_content = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;

    let bytes = match passphrase {
        Some(ref pass) if !pass.trim().is_empty() => {
            bayesian_ssh::services::crypto::encrypt_data(raw_content.as_bytes(), pass)
                .map_err(|e| e.to_string())?
        }
        _ => raw_content.into_bytes(),
    };

    if let Some(path_str) = output_path {
        std::fs::write(&path_str, &bytes).map_err(|e| format!("Failed to write export file: {e}"))?;
        bayesian_ssh::config::enforce_secure_file(std::path::Path::new(&path_str));
        Ok(format!(
            "Exported {} environment(s) and {} connection(s) to {}",
            payload.environments.len(),
            total_connections_count,
            path_str
        ))
    } else {
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

#[tauri::command]
pub fn import_connections_payload(
    file_path: String,
    passphrase: Option<String>,
    no_bastion: bool,
) -> Result<usize, String> {
    let path = std::path::PathBuf::from(&file_path);

    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }

    let raw_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;

    let content_bytes = if raw_bytes.starts_with(b"BSSH") || passphrase.is_some() {
        let pass = passphrase.ok_or_else(|| "Encrypted file requires passphrase".to_string())?;
        bayesian_ssh::services::crypto::decrypt_data(&raw_bytes, &pass)
            .map_err(|e| e.to_string())?
    } else {
        raw_bytes
    };

    let content_str = String::from_utf8_lossy(&content_bytes);

    // Case 1: Try parsing FullBackupPayload (v2 full backup format)
    if let Ok(payload) = serde_json::from_str::<FullBackupPayload>(&content_str) {
        let mut total_imported = 0;

        for env_data in payload.environments {
            let env_cfg = env_data.config;
            let _ = env_cfg.save();

            if let Ok(db) = Database::new(&env_cfg) {
                for mut conn in env_data.connections {
                    if no_bastion {
                        conn.bastion = None;
                        conn.bastion_user = None;
                    }
                    if db.add_connection(&conn).is_ok() {
                        total_imported += 1;
                        for alias in &conn.aliases {
                            let _ = db.add_alias(alias, &conn.id.to_string());
                        }
                    }
                }
            }
        }

        let _ = AppConfig::set_active_env(&payload.active_environment);

        return Ok(total_imported);
    }

    // Case 2: Try parsing Vec<Connection> (v1 legacy format)
    if let Ok(connections) = serde_json::from_str::<Vec<Connection>>(&content_str) {
        let active_config = AppConfig::load(None).map_err(|e| e.to_string())?;
        let db = Database::new(&active_config).map_err(|e| e.to_string())?;
        let mut count = 0;

        for mut conn in connections {
            if no_bastion {
                conn.bastion = None;
                conn.bastion_user = None;
            }
            if db.add_connection(&conn).is_ok() {
                count += 1;
                for alias in &conn.aliases {
                    let _ = db.add_alias(alias, &conn.id.to_string());
                }
            }
        }
        return Ok(count);
    }

    Err("Import file is not a valid Bayesian SSH backup payload".to_string())
}

#[tauri::command]
pub fn fix_security_permissions() -> Result<usize, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    bayesian_ssh::cli::commands::audit::fix_permissions(&config).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone, Debug)]
pub struct PingResultDto {
    pub connection_id: String,
    pub name: String,
    pub host: String,
    pub success: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn ping_all_connections() -> Result<Vec<PingResultDto>, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;
    let connections = database.list_connections(None, false).map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for conn in connections {
        let start = std::time::Instant::now();
        let status = tokio::process::Command::new("ssh")
            .args([
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=3",
                "-p", &conn.port.to_string(),
                &format!("{}@{}", conn.user, conn.host),
                "exit 0",
            ])
            .status()
            .await;

        let duration = start.elapsed().as_millis() as u64;
        let (success, err) = match status {
            Ok(s) if s.success() => (true, None),
            Ok(s) => (false, Some(format!("Exit status: {}", s))),
            Err(e) => (false, Some(e.to_string())),
        };

        results.push(PingResultDto {
            connection_id: conn.id.to_string(),
            name: conn.name,
            host: conn.host,
            success,
            latency_ms: duration,
            error: err,
        });
    }

    Ok(results)
}
