//! SSH key management commands (`bssh key list`, `bssh key generate`, `bssh key copy`)

use crate::config::AppConfig;
use crate::database::Database;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

pub async fn execute_list() -> Result<()> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not resolve home directory"))?
        .join(".ssh");

    println!("SSH Key Inventory (~/.ssh/)");
    println!("--------------------------------------------------");

    if !ssh_dir.exists() {
        println!("No ~/.ssh directory found.");
        return Ok(());
    }

    let entries = fs::read_dir(&ssh_dir)?;
    let mut found_keys = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "pub" {
                    found_keys += 1;
                    let priv_path = path.with_extension("");
                    let filename = path.file_stem().unwrap_or_default().to_string_lossy();

                    let key_content = fs::read_to_string(&path).unwrap_or_default();
                    let parts: Vec<&str> = key_content.split_whitespace().collect();
                    let key_type = parts.first().copied().unwrap_or("unknown");
                    let comment = parts.get(2).copied().unwrap_or("");

                    let fingerprint = if parts.len() >= 2 {
                        if let Ok(raw) = base64::Engine::decode(
                            &base64::engine::general_purpose::STANDARD,
                            parts[1],
                        ) {
                            let mut hasher = Sha256::new();
                            hasher.update(&raw);
                            let hash = hasher.finalize();
                            format!(
                                "SHA256:{}",
                                base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD_NO_PAD,
                                    hash
                                )
                            )
                        } else {
                            "INVALID_BASE64".to_string()
                        }
                    } else {
                        "UNKNOWN".to_string()
                    };

                    let priv_perm = if priv_path.exists() {
                        if let Ok(meta) = fs::metadata(&priv_path) {
                            let mode = meta.permissions().mode() & 0o777;
                            if mode == 0o600 {
                                "0600 (SECURE)".to_string()
                            } else {
                                format!("{:04o} (WARNING: insecure perms)", mode)
                            }
                        } else {
                            "UNKNOWN".to_string()
                        }
                    } else {
                        "PUBLIC_ONLY".to_string()
                    };

                    println!("Key: {}", filename);
                    println!("  Type:        {}", key_type);
                    println!("  Fingerprint: {}", fingerprint);
                    println!("  Comment:     {}", comment);
                    println!("  Private Key: {}", priv_path.display());
                    println!("  Permission:  {}", priv_perm);
                    println!();
                }
            }
        }
    }

    if found_keys == 0 {
        println!("No public SSH key files (*.pub) found in ~/.ssh/");
    }

    Ok(())
}

pub async fn execute_generate(name: String, key_type: Option<String>) -> Result<()> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not resolve home directory"))?
        .join(".ssh");

    fs::create_dir_all(&ssh_dir)?;
    crate::config::enforce_secure_dir(&ssh_dir);

    let key_path = ssh_dir.join(&name);
    if key_path.exists() {
        return Err(anyhow!("Key file already exists at {}", key_path.display()));
    }

    let kt = key_type.unwrap_or_else(|| "ed25519".to_string());
    println!("Generating {} keypair at {}...", kt, key_path.display());

    let status = Command::new("ssh-keygen")
        .arg("-t")
        .arg(&kt)
        .arg("-f")
        .arg(&key_path)
        .arg("-N")
        .arg("")
        .status()?;

    if !status.success() {
        return Err(anyhow!("ssh-keygen failed with status {}", status));
    }

    crate::config::enforce_secure_file(&key_path);
    let pub_key_path = PathBuf::from(format!("{}.pub", key_path.display()));
    if pub_key_path.exists() {
        let mut perms = fs::metadata(&pub_key_path)?.permissions();
        perms.set_mode(0o644);
        let _ = fs::set_permissions(&pub_key_path, perms);
    }

    println!("Keypair generated successfully!");
    println!("Private key: {}", key_path.display());
    println!("Public key:  {}.pub", key_path.display());

    Ok(())
}

pub async fn execute_copy(config: AppConfig, target: String, key: Option<String>) -> Result<()> {
    let database = Database::new(&config)?;
    let conn = database
        .get_connection_or_alias(&target)?
        .ok_or_else(|| anyhow!("Connection '{}' not found", target))?;

    let pub_key_path = if let Some(k) = key {
        let p = PathBuf::from(&k);
        if p.extension().is_some_and(|e| e == "pub") {
            p
        } else {
            PathBuf::from(format!("{}.pub", k))
        }
    } else if let Some(ref k) = conn.key_path {
        PathBuf::from(format!("{}.pub", k))
    } else {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not resolve home directory"))?;
        let default_ed25519 = home.join(".ssh/id_ed25519.pub");
        let default_rsa = home.join(".ssh/id_rsa.pub");
        if default_ed25519.exists() {
            default_ed25519
        } else if default_rsa.exists() {
            default_rsa
        } else {
            return Err(anyhow!("No public key found to copy. Generate one with 'bssh key generate --name id_ed25519'"));
        }
    };

    if !pub_key_path.exists() {
        return Err(anyhow!(
            "Public key file not found: {}",
            pub_key_path.display()
        ));
    }

    let pub_key_content = fs::read_to_string(&pub_key_path)?.trim().to_string();
    println!(
        "Copying public key ({}) to {}@{}...",
        pub_key_path.display(),
        conn.user,
        conn.host
    );

    // Shell-quote the public key content to prevent command injection via
    // crafted key files.  Uses the standard POSIX single-quote escape:
    //   '  →  '\''  (close quote, escaped quote, reopen quote)
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
    let status = Command::new(bin).args(args).status()?;

    if status.success() {
        println!("Public key successfully copied to remote authorized_keys!");
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to copy public key to remote host (exit status: {})",
            status
        ))
    }
}
