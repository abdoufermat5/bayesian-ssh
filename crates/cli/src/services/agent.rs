//! SSH agent socket discovery.
//!
//! Shared service used by the desktop GUI (and CLI) to locate a live, connectable
//! ssh-agent socket via AF_UNIX socket probing of the well-known locations.

/// Return true when `path` points at a live, connectable Unix socket.
///
/// On Unix this verifies the path is a socket *and* that a connection can be
/// established. On non-Unix platforms it falls back to a path-existence check.
#[cfg(unix)]
pub fn is_valid_socket(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixStream;
    let p = std::path::Path::new(path);
    if let Ok(meta) = p.metadata() {
        if meta.file_type().is_socket() {
            return UnixStream::connect(p).is_ok();
        }
    }
    false
}

#[cfg(not(unix))]
pub fn is_valid_socket(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// Resolve the current user's uid as a string (env `UID` then `id -u`).
fn current_uid_string() -> Option<String> {
    if let Ok(uid) = std::env::var("UID") {
        let trimmed = uid.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    #[cfg(unix)]
    {
        let output = std::process::Command::new("id").arg("-u").output().ok()?;
        if output.status.success() {
            let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !uid.is_empty() {
                return Some(uid);
            }
        }
    }

    None
}

/// Walk the well-known places where SSH agent sockets are created and return
/// the first active, connectable SSH agent socket.
#[cfg(unix)]
pub fn find_ssh_agent_socket() -> Option<String> {
    // 1. Glob /tmp/ssh-*/agent.* (the classic openssh-agent pattern - highest priority)
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !name.starts_with("ssh-") {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&dir) {
                for f in files.flatten() {
                    let fp = f.path();
                    let fname = fp
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if fname.starts_with("agent.") {
                        let path_str = fp.to_string_lossy().to_string();
                        if is_valid_socket(&path_str) {
                            return Some(path_str);
                        }
                    }
                }
            }
        }
    }

    // 2. Try common XDG_RUNTIME_DIR & desktop agent socket patterns
    let uid = current_uid_string().unwrap_or_else(|| "0".to_string());
    let runtime_dir = format!("/run/user/{uid}");
    let mut candidates = vec![
        format!("{runtime_dir}/ssh-agent.socket"),
        format!("{runtime_dir}/gcr/ssh"),
        format!("{runtime_dir}/keyring/ssh"),
        format!("{runtime_dir}/openssh_agent"),
        format!("{runtime_dir}/1password/t/agent.sock"),
    ];
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        candidates.push(format!("{home_str}/.1password/agent.sock"));
    }

    for c in &candidates {
        if is_valid_socket(c) {
            return Some(c.clone());
        }
    }

    // 3. Read /proc environ of running processes (excluding gpg-agent for standard key additions)
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let pid_path = entry.path();
            if !pid_path.is_dir() {
                continue;
            }
            let environ_path = pid_path.join("environ");
            if let Ok(data) = std::fs::read(&environ_path) {
                for kv in data.split(|&b| b == 0) {
                    let s = String::from_utf8_lossy(kv);
                    if let Some(val) = s.strip_prefix("SSH_AUTH_SOCK=") {
                        let sock = val.trim();
                        if !sock.is_empty() && !sock.contains("gnupg") && is_valid_socket(sock) {
                            return Some(sock.to_string());
                        }
                    }
                }
            }
        }
    }

    // 4. Fallback to GPG agent SSH socket bridge if no standard SSH agent is running
    let mut gpg_candidates = vec![format!("{runtime_dir}/gnupg/S.gpg-agent.ssh")];
    if let Some(home) = dirs::home_dir() {
        gpg_candidates.push(format!("{}/.gnupg/S.gpg-agent.ssh", home.to_string_lossy()));
    }
    for c in &gpg_candidates {
        if is_valid_socket(c) {
            return Some(c.clone());
        }
    }

    None
}

#[cfg(not(unix))]
pub fn find_ssh_agent_socket() -> Option<String> {
    std::env::var("SSH_AUTH_SOCK")
        .ok()
        .filter(|s| is_valid_socket(s))
}
