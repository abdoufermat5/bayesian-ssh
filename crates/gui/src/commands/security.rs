use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Connection;

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

#[derive(Serialize, Clone, Debug)]
pub struct SshKeyInfo {
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
    pub comment: String,
    pub public_key_path: String,
    pub private_key_path: Option<String>,
    pub private_key_permission: String,
    pub is_secure: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct AuditFindingDto {
    pub severity: String,
    pub title: String,
    pub description: String,
    pub remediation: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AuditReportDto {
    pub score: i32,
    pub grade: String,
    pub rating: String,
    pub findings: Vec<AuditFindingDto>,
    pub total_critical: usize,
    pub total_warning: usize,
    pub total_info: usize,
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn list_ssh_keys() -> Result<Vec<SshKeyInfo>, String> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| "Could not resolve home directory".to_string())?
        .join(".ssh");

    if !ssh_dir.exists() {
        return Ok(Vec::new());
    }

    let mut keys = Vec::new();
    let entries = fs::read_dir(&ssh_dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "pub" {
                    let priv_path = path.with_extension("");
                    let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

                    let key_content = fs::read_to_string(&path).unwrap_or_default();
                    let parts: Vec<&str> = key_content.split_whitespace().collect();
                    let key_type = parts.first().copied().unwrap_or("unknown").to_string();
                    let comment = parts.get(2).copied().unwrap_or("").to_string();

                    let fingerprint = if parts.len() >= 2 {
                        if let Ok(raw) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, parts[1]) {
                            let mut hasher = Sha256::new();
                            hasher.update(&raw);
                            let hash = hasher.finalize();
                            format!("SHA256:{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, hash))
                        } else {
                            "INVALID_BASE64".to_string()
                        }
                    } else {
                        "UNKNOWN".to_string()
                    };

                    let (priv_key_str, priv_perm, is_secure) = if priv_path.exists() {
                        let perm_str = if let Ok(meta) = fs::metadata(&priv_path) {
                            let mode = meta.permissions().mode() & 0o777;
                            if mode == 0o600 {
                                "0600 (SECURE)".to_string()
                            } else {
                                format!("{:04o} (INSECURE)", mode)
                            }
                        } else {
                            "UNKNOWN".to_string()
                        };
                        let secure = perm_str.contains("SECURE");
                        (Some(priv_path.display().to_string()), perm_str, secure)
                    } else {
                        (None, "PUBLIC_ONLY".to_string(), true)
                    };

                    keys.push(SshKeyInfo {
                        name: filename,
                        key_type,
                        fingerprint,
                        comment,
                        public_key_path: path.display().to_string(),
                        private_key_path: priv_key_str,
                        private_key_permission: priv_perm,
                        is_secure,
                    });
                }
            }
        }
    }

    Ok(keys)
}

#[tauri::command]
pub fn generate_ssh_key(name: String, key_type: Option<String>) -> Result<SshKeyInfo, String> {
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

    list_ssh_keys()?
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
        let default_ed25519 = dirs::home_dir().unwrap().join(".ssh/id_ed25519.pub");
        let default_rsa = dirs::home_dir().unwrap().join(".ssh/id_rsa.pub");
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

    let remote_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        pub_key_content
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
pub fn run_security_audit() -> Result<AuditReportDto, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;
    let connections = database.list_connections(None, false).map_err(|e| e.to_string())?;

    let mut findings = Vec::new();

    if config.transport.strict_host_key_checking == "off" {
        findings.push(AuditFindingDto {
            severity: "critical".into(),
            title: "StrictHostKeyChecking Disabled".into(),
            description: "Global strict host key checking is set to 'off'. This exposes connections to Man-in-the-Middle (MITM) attacks.".into(),
            remediation: "Set strict_host_key_checking to 'accept-new' or 'strict' in settings.".into(),
        });
    }

    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh");
    if let Ok(meta) = fs::metadata(&config_dir) {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            findings.push(AuditFindingDto {
                severity: "warning".into(),
                title: "Config Directory Permissions Overly Permissive".into(),
                description: format!("Directory {} mode is {:04o} (expected 0700). Other local users may access settings.", config_dir.display(), mode),
                remediation: format!("Run 'chmod 700 {}'", config_dir.display()),
            });
        }
    }

    if let Ok(meta) = fs::metadata(&config.database_path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            findings.push(AuditFindingDto {
                severity: "warning".into(),
                title: "Database File Permissions Overly Permissive".into(),
                description: format!("Database file {} mode is {:04o} (expected 0600). Other local users may view stored server records.", config.database_path.display(), mode),
                remediation: format!("Run 'chmod 600 {}'", config.database_path.display()),
            });
        }
    }

    if let Some(ssh_dir) = dirs::home_dir().map(|h| h.join(".ssh")) {
        if ssh_dir.exists() {
            if let Ok(entries) = fs::read_dir(&ssh_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_none_or(|ext| ext != "pub" && ext != "known_hosts" && ext != "config") {
                        if let Ok(meta) = fs::metadata(&path) {
                            let mode = meta.permissions().mode() & 0o777;
                            if mode != 0o600 {
                                findings.push(AuditFindingDto {
                                    severity: "warning".into(),
                                    title: format!("Insecure Private Key Permissions: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                                    description: format!("Key file {} mode is {:04o} (expected 0600).", path.display(), mode),
                                    remediation: format!("Run 'chmod 600 {}'", path.display()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let now = chrono::Utc::now();
    let mut password_only = 0;
    let mut stale = 0;

    for conn in &connections {
        if conn.key_path.is_none() && !conn.use_kerberos {
            password_only += 1;
        }
        if let Some(ref last_used) = conn.last_used {
            if (now - *last_used).num_days() > 90 {
                stale += 1;
            }
        }
    }

    if password_only > 0 {
        findings.push(AuditFindingDto {
            severity: "info".into(),
            title: format!("{} Connection(s) Missing Identity Keys", password_only),
            description: "Connections without explicit SSH identity keys or Kerberos rely on password auth or agent fallback.".into(),
            remediation: "Assign identity key files or generate keypairs via the Key Manager.".into(),
        });
    }

    if stale > 0 {
        findings.push(AuditFindingDto {
            severity: "info".into(),
            title: format!("{} Stale Connection(s) (>90 days unused)", stale),
            description: "Unused connections increase attack surface and clutter server list.".into(),
            remediation: "Review and remove inactive connections.".into(),
        });
    }

    let mut score = 100i32;
    let mut total_critical = 0;
    let mut total_warning = 0;
    let mut total_info = 0;

    for f in &findings {
        match f.severity.as_str() {
            "critical" => {
                score -= 25;
                total_critical += 1;
            }
            "warning" => {
                score -= 10;
                total_warning += 1;
            }
            _ => {
                score -= 2;
                total_info += 1;
            }
        }
    }
    score = score.max(0);

    let (grade, rating) = match score {
        90..=100 => ("A+", "EXCELLENT"),
        80..=89 => ("A", "GOOD"),
        70..=79 => ("B", "MODERATE"),
        60..=69 => ("C", "NEEDS IMPROVEMENT"),
        _ => ("F", "CRITICAL RISK"),
    };

    Ok(AuditReportDto {
        score,
        grade: grade.into(),
        rating: rating.into(),
        findings,
        total_critical,
        total_warning,
        total_info,
    })
}

#[tauri::command]
pub fn export_connections_payload(
    output_path: Option<String>,
    passphrase: Option<String>,
    format: Option<String>,
    tag: Option<String>,
) -> Result<String, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;
    let connections = database
        .list_connections(tag.as_deref(), false)
        .map_err(|e| e.to_string())?;

    let format_str = format.unwrap_or_else(|| "json".to_string());
    let raw_content = match format_str.as_str() {
        "json" => serde_json::to_string_pretty(&connections).map_err(|e| e.to_string())?,
        _ => serde_json::to_string_pretty(&connections).map_err(|e| e.to_string())?,
    };

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
            "Exported {} connections to {}",
            connections.len(),
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
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let database = Database::new(&config).map_err(|e| e.to_string())?;
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

    if let Ok(connections) = serde_json::from_str::<Vec<Connection>>(&content_str) {
        let mut count = 0;
        for mut conn in connections {
            if no_bastion {
                conn.bastion = None;
                conn.bastion_user = None;
            }
            database.add_connection(&conn).map_err(|e| e.to_string())?;
            count += 1;
        }
        return Ok(count);
    }

    Err("Import file is not a valid JSON connection backup or payload".to_string())
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
