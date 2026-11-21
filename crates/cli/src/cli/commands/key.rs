//! SSH key management commands (`bssh key list`, `bssh key generate`, `bssh key copy`)

use crate::config::AppConfig;
use crate::database::Database;
use crate::ui::{Glyph, KvList, Style, Table, Tone, heading};
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

pub async fn execute_list() -> Result<()> {
    let style = Style::terminal();
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not resolve home directory"))?
        .join(".ssh");

    println!("{}", heading(&style, "SSH Key Inventory", Some("~/.ssh/")));

    if !ssh_dir.exists() {
        println!("{} No ~/.ssh directory found.", style.mark(Glyph::Dot));
        return Ok(());
    }

    let mut pub_paths: Vec<PathBuf> = fs::read_dir(&ssh_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "pub"))
        .collect();
    pub_paths.sort();

    if pub_paths.is_empty() {
        println!(
            "{} No public SSH key files (*.pub) found in ~/.ssh/",
            style.mark(Glyph::Dot)
        );
        return Ok(());
    }

    let mut table = Table::new(&["KEY", "TYPE", "FINGERPRINT", "PERMS", "COMMENT"]);

    for path in &pub_paths {
        let priv_path = path.with_extension("");
        let filename = path.file_stem().unwrap_or_default().to_string_lossy();

        let key_content = fs::read_to_string(path).unwrap_or_default();
        let parts: Vec<&str> = key_content.split_whitespace().collect();
        let key_type = parts.first().copied().unwrap_or("unknown");
        let comment = parts.get(2).copied().unwrap_or("");

        let fingerprint = if parts.len() >= 2 {
            if let Ok(raw) =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, parts[1])
            {
                let mut hasher = Sha256::new();
                hasher.update(&raw);
                let hash = hasher.finalize();
                format!(
                    "SHA256:{}",
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, hash)
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
                if mode & 0o077 == 0 {
                    format!(
                        "{} {}",
                        style.mark(Glyph::Ok),
                        style.paint(Tone::Ok, &format!("{mode:04o} secure"))
                    )
                } else {
                    format!(
                        "{} {}",
                        style.mark(Glyph::Ask),
                        style.paint(Tone::Warn, &format!("{mode:04o} insecure"))
                    )
                }
            } else {
                style.paint(Tone::Dim, "unknown")
            }
        } else {
            style.paint(Tone::Dim, "public only")
        };

        table.row(vec![
            style.paint(Tone::Strong, &filename),
            key_type.to_string(),
            style.paint(Tone::Dim, &fingerprint),
            priv_perm,
            style.paint(Tone::Dim, comment),
        ]);
    }

    println!("{}", table.render(&style));
    Ok(())
}

pub async fn execute_generate(name: String, key_type: Option<String>) -> Result<()> {
    let style = Style::terminal();
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not resolve home directory"))?
        .join(".ssh");

    fs::create_dir_all(&ssh_dir)?;
    crate::config::enforce_secure_dir(&ssh_dir);

    // The key must land in ~/.ssh: reject separators/`..`/absolute paths
    // that would let `join` escape the directory.
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(anyhow!(
            "Invalid key name '{}': use a plain file name such as id_ed25519",
            name
        ));
    }
    let key_path = ssh_dir.join(&name);
    if key_path.exists() {
        return Err(anyhow!("Key file already exists at {}", key_path.display()));
    }

    let kt = key_type.unwrap_or_else(|| "ed25519".to_string());
    println!(
        "{} Generating {} keypair at {}...",
        style.mark(Glyph::Running),
        style.paint(Tone::Strong, &kt),
        key_path.display()
    );

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

    println!("{} Keypair generated.", style.mark(Glyph::Ok));
    let mut kv = KvList::new();
    kv.push("Private key", key_path.display().to_string());
    kv.push("Public key", format!("{}.pub", key_path.display()));
    println!("{}", kv.render(&style));

    Ok(())
}

pub async fn execute_copy(config: AppConfig, target: String, key: Option<String>) -> Result<()> {
    let style = Style::terminal();
    let database = Database::new(&config)?;
    let conn = database
        .connection_or_alias(&target)?
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
            return Err(anyhow!(
                "No public key found to copy. Generate one with 'bssh key generate --name id_ed25519'"
            ));
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
        "{} Copying public key ({}) to {}@{}...",
        style.mark(Glyph::Running),
        pub_key_path.display(),
        conn.user,
        conn.host
    );

    // Shell-quote the public key content to prevent command injection via
    // crafted key files.  Uses the standard POSIX single-quote escape:
    //   '  →  '\''  (close quote, escaped quote, reopen quote)
    let quoted_key = format!("'{}'", pub_key_content.replace('\'', "'\"'\"'"));
    let remote_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && printf '%s\\n' {quoted_key} >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys"
    );

    let mut cmd = Command::new("ssh");
    if let Some(sock) = &config.auth.agent_socket {
        cmd.env(crate::services::auth::SSH_AUTH_SOCK, sock);
    }
    cmd.arg("-p").arg(conn.port.to_string());
    if let Some(bastion) = &conn.bastion {
        let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
        cmd.arg("-J").arg(format!("{}@{}", bu, bastion));
    }
    // `--` ends option parsing so a stored user/host beginning with `-`
    // cannot be interpreted as an ssh option (e.g. `-oProxyCommand=...`).
    cmd.arg("--")
        .arg(format!("{}@{}", conn.user, conn.host))
        .arg(remote_cmd);
    let status = cmd.status()?;

    if status.success() {
        println!(
            "{} Public key copied to remote authorized_keys.",
            style.mark(Glyph::Ok)
        );
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to copy public key to remote host (exit status: {})",
            status
        ))
    }
}
