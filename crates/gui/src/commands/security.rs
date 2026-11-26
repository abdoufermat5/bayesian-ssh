use super::{AllowedPaths, CommandError};
use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Connection;
use bayesian_ssh::services::security;

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::State;
use uuid::Uuid;

const RESERVED_SSH_FILENAMES: &[&str] = &[
    "config",
    "authorized_keys",
    "known_hosts",
    "known_hosts.old",
];

fn validate_key_name(raw: &str) -> Result<String, CommandError> {
    // Reject anything that is not a plain file name: this blocks path
    // traversal (e.g. "../../etc/cron.d/x") and absolute paths.
    let name = raw.trim().to_string();
    if name.is_empty()
        || name.len() > 128
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
    {
        return Err(
            "Key name may only contain letters, digits, '_', '-' and '.' (no paths).".into(),
        );
    }
    let lower = name.to_ascii_lowercase();
    if RESERVED_SSH_FILENAMES.contains(&lower.as_str()) {
        return Err(format!("'{name}' is a reserved SSH file name and cannot be used.").into());
    }
    Ok(name)
}

fn validate_ssh_public_key(content: &str) -> Result<(), CommandError> {
    if content.is_empty()
        || content.contains('\n')
        || content.contains('\r')
        || content.contains('\0')
    {
        return Err("Public key must be a single non-empty line.".into());
    }
    let mut parts = content.split_whitespace();
    let Some(key_type) = parts.next() else {
        return Err("Public key is empty.".into());
    };
    let Some(key_blob) = parts.next() else {
        return Err("Public key is missing key material.".into());
    };
    let valid_prefix = key_type.starts_with("ssh-")
        || key_type.starts_with("ecdsa-sha2-")
        || key_type.starts_with("sk-ssh-")
        || key_type.starts_with("sk-ecdsa-sha2-");
    if !valid_prefix {
        return Err(format!("Unsupported or invalid SSH public key type '{key_type}'.").into());
    }
    if key_blob.is_empty()
        || !key_blob
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return Err("Public key contains invalid base64 key material.".into());
    }
    Ok(())
}

/// Write `bytes` atomically to `dest` with `0o600` permissions from creation
/// time (no TOCTOU window where the file is world-readable).
fn write_export_atomic_0600(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let tmp_name = format!(".bssh-export-{}.tmp", Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let write_res = (|| -> std::io::Result<()> {
        let mut file = opts.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write export file: {e}"));
    }

    if let Err(e) = fs::rename(&tmp_path, dest) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write export file: {e}"));
    }

    Ok(())
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub async fn list_ssh_keys(
    allowed: State<'_, AllowedPaths>,
) -> Result<Vec<security::SshKeyInfo>, CommandError> {
    let keys = tokio::task::spawn_blocking(security::scan_ssh_keys)
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;
    for key in &keys {
        let pub_path = Path::new(&key.public_key_path);
        if pub_path.exists() {
            allowed.allow_read_file(pub_path);
        }
    }
    Ok(keys)
}

// ssh-keygen can block (RSA generation, touch prompts for *-sk keys): keep it
// off the Tokio async worker threads.
#[tauri::command]
pub async fn generate_ssh_key(
    name: String,
    key_type: Option<String>,
) -> Result<security::SshKeyInfo, CommandError> {
    let name = validate_key_name(&name)?;
    let kt = key_type.unwrap_or_else(|| "ed25519".to_string());
    if !["ed25519", "rsa", "ecdsa", "dsa", "ed25519-sk", "ecdsa-sk"].contains(&kt.as_str()) {
        return Err(format!("Unsupported key type: {kt}").into());
    }

    tokio::task::spawn_blocking(move || {
        let ssh_dir = dirs::home_dir()
            .ok_or_else(|| CommandError::from("Could not resolve home directory"))?
            .join(".ssh");

        fs::create_dir_all(&ssh_dir)?;
        bayesian_ssh::config::enforce_secure_dir(&ssh_dir);

        let key_path = ssh_dir.join(&name);
        let pub_key_path = PathBuf::from(format!("{}.pub", key_path.display()));
        if key_path.exists() {
            return Err(format!("Key file already exists at {}", key_path.display()).into());
        }
        if pub_key_path.exists() {
            return Err(format!(
                "Public key file already exists at {}",
                pub_key_path.display()
            )
            .into());
        }

        let status = Command::new("ssh-keygen")
            .arg("-t")
            .arg(&kt)
            .arg("-f")
            .arg(&key_path)
            .arg("-N")
            .arg("")
            .status()
            .map_err(|e| CommandError::from(format!("Failed to run ssh-keygen: {e}")))?;

        if !status.success() {
            return Err(format!("ssh-keygen exited with status {status}").into());
        }

        bayesian_ssh::config::enforce_secure_file(&key_path);
        #[cfg(unix)]
        if pub_key_path.exists() {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&pub_key_path)?.permissions();
            perms.set_mode(0o644);
            let _ = fs::set_permissions(&pub_key_path, perms);
        }

        security::scan_ssh_keys()?
            .into_iter()
            .find(|k| k.name == name)
            .ok_or_else(|| CommandError::from("Key generated but details could not be loaded"))
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

fn is_in_ssh_dir_pub(path: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let Ok(canon_ssh) = home.join(".ssh").canonicalize() else {
        return false;
    };
    let Ok(canon_path) = path.canonicalize() else {
        return false;
    };
    canon_path.starts_with(&canon_ssh) && canon_path.extension().is_some_and(|e| e == "pub")
}

// Runs ssh against a remote host (can block for the whole connect timeout),
// so keep it off the Tokio async worker threads.
#[tauri::command]
pub async fn copy_ssh_key_to_target(
    allowed: State<'_, AllowedPaths>,
    target: String,
    key_path: Option<String>,
) -> Result<String, CommandError> {
    let (conn, pub_key_path) = tokio::task::spawn_blocking(move || -> Result<_, CommandError> {
        let config = AppConfig::load(None)?;
        let database = Database::new(&config)?;

        let conn = database
            .connection_or_alias(&target)?
            .ok_or_else(|| format!("Connection '{target}' not found"))?;
        if conn.pending_change.is_some() {
            return Err(bayesian_ssh::services::pending::refusal(&conn).into());
        }

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
            let home =
                dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string())?;
            let default_ed25519 = home.join(".ssh/id_ed25519.pub");
            let default_rsa = home.join(".ssh/id_rsa.pub");
            if default_ed25519.exists() {
                default_ed25519
            } else if default_rsa.exists() {
                default_rsa
            } else {
                return Err(
                    "No public key found. Generate one first using the Key Manager.".into(),
                );
            }
        };

        if !pub_key_path.exists() {
            return Err(format!("Public key file not found: {}", pub_key_path.display()).into());
        }

        Ok((conn, pub_key_path))
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;

    // Allow public keys inside ~/.ssh/ or explicitly picked via native dialogs.
    let verified_path = if is_in_ssh_dir_pub(&pub_key_path) {
        pub_key_path
            .canonicalize()
            .map_err(|e| CommandError::from(format!("Public key file unavailable: {e}")))?
    } else {
        allowed.check_read_file(&pub_key_path)?
    };

    tokio::task::spawn_blocking(move || {
        let pub_key_content = fs::read_to_string(&verified_path)?.trim().to_string();
        validate_ssh_public_key(&pub_key_content)?;

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
        // `--` keeps a stored user/host starting with '-' from being parsed as an ssh option.
        ssh_argv.push("--".to_string());
        ssh_argv.push(format!("{}@{}", conn.user, conn.host));
        ssh_argv.push(remote_cmd);

        let (bin, args) = ssh_argv
            .split_first()
            .expect("ssh_argv starts with the ssh binary");
        let status = Command::new(bin)
            .args(args)
            .status()
            .map_err(|e| CommandError::from(format!("Failed to execute ssh: {e}")))?;

        if status.success() {
            Ok(format!(
                "Public key ({}) successfully copied to {}@{}",
                verified_path.display(),
                conn.user,
                conn.host
            ))
        } else {
            Err(format!("SSH failed with exit code {}", status).into())
        }
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command(async)]
pub fn run_security_audit() -> Result<security::AuditReportDto, CommandError> {
    let config = AppConfig::load(None)?;
    let database = Database::new(&config)?;
    let connections = database.list_connections(None, false)?;

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
pub async fn export_connections_payload(
    allowed: State<'_, AllowedPaths>,
    output_path: Option<String>,
    passphrase: Option<String>,
    _format: Option<String>,
    tag: Option<String>,
) -> Result<String, CommandError> {
    let verified_output = match output_path {
        Some(path_str) => Some((
            path_str.clone(),
            allowed.check_write_file(Path::new(&path_str))?,
        )),
        None => None,
    };

    tokio::task::spawn_blocking(move || {
        let global_config = AppConfig::load(None)?;
        let active_env = AppConfig::active_env();
        let env_list = crate::commands::env::list_environments().unwrap_or_default();

        let mut backup_environments = Vec::new();
        let mut total_connections_count = 0;

        for env_info in env_list {
            let env_cfg = match AppConfig::load(Some(env_info.name.clone())) {
                Ok(cfg) => cfg,
                Err(_) => AppConfig::default_for_env(&env_info.name),
            };

            let mut env_connections = Vec::new();
            if let Ok(db) = Database::new(&env_cfg)
                && let Ok(conns) = db.list_connections(tag.as_deref(), false)
            {
                total_connections_count += conns.len();
                env_connections = conns;
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

        let raw_content = serde_json::to_string_pretty(&payload)?;

        let bytes = match passphrase {
            Some(ref pass) if !pass.trim().is_empty() => {
                bayesian_ssh::services::crypto::encrypt_data(raw_content.as_bytes(), pass)?
            }
            _ => raw_content.into_bytes(),
        };

        if let Some((path_str, verified_path)) = verified_output {
            write_export_atomic_0600(&verified_path, &bytes)?;
            Ok(format!(
                "Exported {} environment(s) and {} connection(s) to {}",
                payload.environments.len(),
                total_connections_count,
                path_str
            ))
        } else {
            Ok(String::from_utf8_lossy(&bytes).to_string())
        }
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command(async)]
pub fn import_connections_payload(
    file_path: String,
    passphrase: Option<String>,
    no_bastion: bool,
) -> Result<usize, CommandError> {
    let path = std::path::PathBuf::from(&file_path);

    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path).into());
    }

    let raw_bytes = std::fs::read(&path)?;

    // Only payloads carrying the encryption header are decrypted; a passphrase
    // typed for a plain JSON backup must not turn a valid import into an error.
    let content_bytes = if raw_bytes.starts_with(b"BSSH") {
        let pass = passphrase.ok_or_else(|| "Encrypted file requires passphrase".to_string())?;
        bayesian_ssh::services::crypto::decrypt_data(&raw_bytes, &pass)?
    } else {
        raw_bytes
    };

    let content_str = String::from_utf8_lossy(&content_bytes);

    // Case 1: Try parsing FullBackupPayload (v2 full backup format)
    if let Ok(payload) = serde_json::from_str::<FullBackupPayload>(&content_str) {
        let mut total_imported = 0;

        for env_data in payload.environments {
            // `AppConfig::environment` is serde-skipped (so it deserializes as
            // "default") and `database_path` is an absolute path from the
            // exporting machine. Rebind both to the named environment so each
            // one lands in its own directory and a crafted backup cannot point
            // the database (or the config write) outside the config tree.
            if AppConfig::validate_env_name(&env_data.name).is_err() {
                continue;
            }
            let mut env_cfg = env_data.config;
            env_cfg.database_path = AppConfig::default_for_env(&env_data.name).database_path;
            env_cfg.environment = env_data.name;
            let _ = env_cfg.save();

            if let Ok(db) = Database::new(&env_cfg) {
                for mut conn in env_data.connections {
                    if no_bastion {
                        conn.bastion = None;
                        conn.bastion_user = None;
                    }
                    if !is_importable(&conn) {
                        continue;
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
        let active_config = AppConfig::load(None)?;
        let db = Database::new(&active_config)?;
        let mut count = 0;

        for mut conn in connections {
            if no_bastion {
                conn.bastion = None;
                conn.bastion_user = None;
            }
            if !is_importable(&conn) {
                continue;
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

    Err("Import file is not a valid Bayesian SSH backup payload".into())
}

/// Apply the same field validation as the add/edit dialogs to connections
/// coming from a backup file, which would otherwise reach the ssh argv
/// unchecked (e.g. a user of "-oProxyCommand=…").
fn is_importable(conn: &Connection) -> bool {
    conn.validate().is_ok()
}

#[tauri::command(async)]
pub fn fix_security_permissions() -> Result<usize, CommandError> {
    let config = AppConfig::load(None)?;
    bayesian_ssh::cli::commands::audit::fix_permissions(&config).map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_name_validation_rejects_reserved_and_traversal_names() {
        assert!(validate_key_name("id_ed25519_work").is_ok());
        assert!(validate_key_name("my-key.1").is_ok());

        for reserved in [
            "config",
            "CONFIG",
            "authorized_keys",
            "known_hosts",
            "known_hosts.old",
        ] {
            assert!(
                validate_key_name(reserved).is_err(),
                "reserved name '{reserved}' should be rejected"
            );
        }

        assert!(validate_key_name("").is_err());
        assert!(validate_key_name("../id_rsa").is_err());
        assert!(validate_key_name("sub/key").is_err());
    }

    #[test]
    fn ssh_public_key_validation_accepts_single_line_keys_only() {
        assert!(
            validate_ssh_public_key(
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyDataHere user@host"
            )
            .is_ok()
        );
        assert!(validate_ssh_public_key("ssh-ed25519 AAAAC3==\nssh-ed25519 BBB=").is_err());
        assert!(validate_ssh_public_key("not-a-key AAAAC3==").is_err());
        assert!(validate_ssh_public_key("ssh-ed25519").is_err());
        assert!(validate_ssh_public_key("ssh-ed25519 bad$chars").is_err());
    }
}
