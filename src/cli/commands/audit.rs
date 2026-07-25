//! Security auditor subcommand (`bssh audit`)

use crate::config::AppConfig;
use crate::database::Database;
use anyhow::Result;
use chrono::Utc;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum AuditSeverity {
    Critical,
    Warning,
    Info,
}

pub struct AuditFinding {
    pub severity: AuditSeverity,
    pub title: String,
    pub description: String,
    pub remediation: String,
}

pub async fn execute(config: AppConfig) -> Result<()> {
    let mut findings: Vec<AuditFinding> = Vec::new();
    let database = Database::new(&config)?;
    let connections = database.list_connections(None, false)?;

    println!("==================================================");
    println!("        Bayesian SSH Security Audit Report        ");
    println!("==================================================");
    println!("Environment: {}\n", config.environment);

    // 1. Audit Transport Security Settings
    if config.transport.strict_host_key_checking == "off" {
        findings.push(AuditFinding {
            severity: AuditSeverity::Critical,
            title: "StrictHostKeyChecking Disabled".into(),
            description: "Global strict host key checking is set to 'off'. This exposes connections to Man-in-the-Middle (MITM) attacks.".into(),
            remediation: "Set strict_host_key_checking to 'accept-new' or 'strict' in config.".into(),
        });
    }

    // 2. Audit Config & DB Permissions
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh");
    if let Ok(meta) = fs::metadata(&config_dir) {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            findings.push(AuditFinding {
                severity: AuditSeverity::Warning,
                title: "Config Directory Permissions Overly Permissive".into(),
                description: format!("Directory {} mode is {:04o} (expected 0700). Other local users may read/write configuration.", config_dir.display(), mode),
                remediation: format!("Run 'chmod 700 {}'", config_dir.display()),
            });
        }
    }

    if let Ok(meta) = fs::metadata(&config.database_path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            findings.push(AuditFinding {
                severity: AuditSeverity::Warning,
                title: "Database File Permissions Overly Permissive".into(),
                description: format!("Database file {} mode is {:04o} (expected 0600). Other local users may view connection records.", config.database_path.display(), mode),
                remediation: format!("Run 'chmod 600 {}'", config.database_path.display()),
            });
        }
    }

    // 3. Audit Local SSH Private Key Permissions
    if let Some(ssh_dir) = dirs::home_dir().map(|h| h.join(".ssh")) {
        if ssh_dir.exists() {
            if let Ok(entries) = fs::read_dir(&ssh_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().is_none_or(|ext| {
                            ext != "pub" && ext != "known_hosts" && ext != "config"
                        })
                    {
                        if let Ok(meta) = fs::metadata(&path) {
                            let mode = meta.permissions().mode() & 0o777;
                            if mode != 0o600 {
                                findings.push(AuditFinding {
                                    severity: AuditSeverity::Warning,
                                    title: format!(
                                        "Insecure Private Key Permissions: {}",
                                        path.file_name().unwrap_or_default().to_string_lossy()
                                    ),
                                    description: format!(
                                        "Key file {} mode is {:04o} (expected 0600).",
                                        path.display(),
                                        mode
                                    ),
                                    remediation: format!("Run 'chmod 600 {}'", path.display()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Audit Connections Setup
    let now = Utc::now();
    let mut password_only_count = 0;
    let mut stale_count = 0;

    for conn in &connections {
        // Check authentication mode
        if conn.key_path.is_none() && !conn.use_kerberos {
            password_only_count += 1;
        }

        // Check stale connections (>90 days)
        if let Some(ref last_used) = conn.last_used {
            let days = (now - *last_used).num_days();
            if days > 90 {
                stale_count += 1;
            }
        }
    }

    if password_only_count > 0 {
        findings.push(AuditFinding {
            severity: AuditSeverity::Info,
            title: format!("{} Connection(s) Relying on Interactive Password Auth", password_only_count),
            description: "Connections without explicit SSH identity keys or Kerberos rely on password authentication or agent fallback.".into(),
            remediation: "Assign explicit SSH keys or generate new keypairs via 'bssh key generate'.".into(),
        });
    }

    if stale_count > 0 {
        findings.push(AuditFinding {
            severity: AuditSeverity::Info,
            title: format!("{} Stale Connection(s) Unused for >90 Days", stale_count),
            description:
                "Unused connection entries increase attack surface and clutter session inventory."
                    .into(),
            remediation: "Review and remove inactive servers via 'bssh remove <name>'.".into(),
        });
    }

    // 5. Compute Security Score
    let mut score = 100i32;
    for finding in &findings {
        match finding.severity {
            AuditSeverity::Critical => score -= 25,
            AuditSeverity::Warning => score -= 10,
            AuditSeverity::Info => score -= 2,
        }
    }
    score = score.max(0);

    let (grade, color_label) = match score {
        90..=100 => ("A+", "EXCELLENT"),
        80..=89 => ("A", "GOOD"),
        70..=79 => ("B", "MODERATE"),
        60..=69 => ("C", "NEEDS IMPROVEMENT"),
        _ => ("F", "CRITICAL RISK"),
    };

    println!(
        "Security Score: {}/100 (Grade: {}, Rating: {})\n",
        score, grade, color_label
    );
    println!("Audit Breakdown (Total Findings: {})\n", findings.len());

    if findings.is_empty() {
        println!("No security issues found! Your SSH environment is cleanly configured.");
    } else {
        for (idx, finding) in findings.iter().enumerate() {
            let sev_str = match finding.severity {
                AuditSeverity::Critical => "[CRITICAL]",
                AuditSeverity::Warning => "[WARNING]",
                AuditSeverity::Info => "[INFO]",
            };
            println!("{}. {} {}", idx + 1, sev_str, finding.title);
            println!("   Description: {}", finding.description);
            println!("   Remediation: {}\n", finding.remediation);
        }
    }

    Ok(())
}
