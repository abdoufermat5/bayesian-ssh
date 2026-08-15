//! Security utilities shared by the CLI and desktop GUI: SSH key scanning,
//! key fingerprinting and the security audit report builder.
//!
//! Fingerprinting is delegated to [`crate::services::known_hosts::fingerprint_sha256`]
//! so the SHA-256/`SHA256:` encoding logic has a single definition.

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::known_hosts::fingerprint_sha256;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// A single SSH keypair discovered under `~/.ssh/`, serialized for the GUI.
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

/// Scan `~/.ssh/` for `*.pub` keypairs and return their info.
pub fn scan_ssh_keys() -> Result<Vec<SshKeyInfo>, String> {
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
                            fingerprint_sha256(&raw)
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

/// A single audit finding, serialized for the GUI.
#[derive(Serialize, Clone, Debug)]
pub struct AuditFindingDto {
    pub severity: String,
    pub title: String,
    pub description: String,
    pub remediation: String,
}

/// Full security audit report, serialized for the GUI.
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

/// Build a security audit report from the active configuration and connections.
pub fn build_audit_report(config: &AppConfig, connections: &[Connection]) -> AuditReportDto {
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

    for conn in connections {
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

    AuditReportDto {
        score,
        grade: grade.into(),
        rating: rating.into(),
        findings,
        total_critical,
        total_warning,
        total_info,
    }
}
