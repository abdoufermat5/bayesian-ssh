//! SSH authentication helpers for the native russh transport.
//!
//! Provides:
//! - `candidate_key_paths()` — ordered list of key files to try
//! - `AgentClient` wrapper for ssh-agent protocol (via `russh-keys`)

#![allow(dead_code)]

use crate::config::AuthConfig;
use crate::models::Connection;
use russh_keys::key::KeyPair;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("failed to load key {path}: {source}")]
    KeyLoad {
        path: PathBuf,
        source: russh_keys::Error,
    },
    #[error("agent error: {0}")]
    Agent(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ──────────────────────────────────────────────────────────────────────────────
// Key discovery
// ──────────────────────────────────────────────────────────────────────────────

/// Standard key file names to probe under `~/.ssh/`, in preference order.
const DEFAULT_KEY_NAMES: &[&str] = &["id_ed25519", "id_ecdsa", "id_rsa", "id_dsa"];

/// Return the ordered list of key paths to attempt.
///
/// Priority:
/// 1. Paths from `AuthConfig.identity_files` (user-configured overrides)
/// 2. `Connection.identity_file` (per-connection `-i` flag equivalent)
/// 3. Standard keys in `~/.ssh/` that actually exist on disk
pub fn candidate_key_paths(conn: &Connection, cfg: &AuthConfig) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // 1. Global config overrides
    paths.extend(cfg.identity_files.iter().cloned());

    // 2. Per-connection key file (equivalent to `-i` flag)
    if let Some(ref key_path) = conn.key_path {
        paths.push(PathBuf::from(key_path));
    }

    // 3. Standard key files that exist on disk
    if let Some(ssh_dir) = ssh_dir() {
        for name in DEFAULT_KEY_NAMES {
            let p = ssh_dir.join(name);
            if p.exists() {
                paths.push(p);
            }
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));

    paths
}

fn ssh_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Key loading
// ──────────────────────────────────────────────────────────────────────────────

/// Try to load a `KeyPair` from `path` without a passphrase.
/// Returns `None` if the key is encrypted (caller should try agent or prompt).
pub fn try_load_key_no_passphrase(path: &Path) -> Result<Option<KeyPair>, AuthError> {
    match russh_keys::load_secret_key(path, None) {
        Ok(kp) => Ok(Some(kp)),
        Err(russh_keys::Error::KeyIsEncrypted) => Ok(None),
        Err(e) => Err(AuthError::KeyLoad {
            path: path.to_owned(),
            source: e,
        }),
    }
}

/// Load a `KeyPair` from `path` using `passphrase`.
pub fn load_key_with_passphrase(path: &Path, passphrase: &str) -> Result<KeyPair, AuthError> {
    russh_keys::load_secret_key(path, Some(passphrase)).map_err(|e| AuthError::KeyLoad {
        path: path.to_owned(),
        source: e,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Agent socket
// ──────────────────────────────────────────────────────────────────────────────

/// Resolve the ssh-agent socket path.
///
/// Priority: `AuthConfig.agent_socket` → `SSH_AUTH_SOCK` env var.
pub fn agent_socket_path(cfg: &AuthConfig) -> Option<PathBuf> {
    if let Some(ref p) = cfg.agent_socket {
        return Some(p.clone());
    }
    std::env::var("SSH_AUTH_SOCK").ok().map(PathBuf::from)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthConfig;
    use crate::models::Connection;
    fn minimal_conn() -> Connection {
        Connection::new(
            "test".into(),
            "host.example".into(),
            "user".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    #[test]
    fn config_override_comes_first() {
        let cfg = AuthConfig {
            identity_files: vec![PathBuf::from("/tmp/mykey")],
            use_agent: true,
            agent_socket: None,
        };
        let conn = minimal_conn();
        let paths = candidate_key_paths(&conn, &cfg);
        assert_eq!(paths[0], PathBuf::from("/tmp/mykey"));
    }

    #[test]
    fn per_connection_key_included() {
        let cfg = AuthConfig::default();
        let mut conn = minimal_conn();
        conn.key_path = Some("/home/user/.ssh/custom_ed25519".into());
        let paths = candidate_key_paths(&conn, &cfg);
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains("custom_ed25519")));
    }

    #[test]
    fn no_duplicates_in_candidate_paths() {
        let key = PathBuf::from("/tmp/deduped_key");
        let cfg = AuthConfig {
            identity_files: vec![key.clone(), key.clone()],
            use_agent: true,
            agent_socket: None,
        };
        let conn = minimal_conn();
        let paths = candidate_key_paths(&conn, &cfg);
        let count = paths.iter().filter(|p| **p == key).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn agent_socket_from_env() {
        // Set env var and verify it is used when no config override
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/agent.sock");
        let cfg = AuthConfig::default();
        let sock = agent_socket_path(&cfg);
        assert_eq!(sock, Some(PathBuf::from("/tmp/agent.sock")));
        std::env::remove_var("SSH_AUTH_SOCK");
    }

    #[test]
    fn agent_socket_config_overrides_env() {
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/env.sock");
        let cfg = AuthConfig {
            agent_socket: Some(PathBuf::from("/tmp/cfg.sock")),
            ..Default::default()
        };
        let sock = agent_socket_path(&cfg);
        assert_eq!(sock, Some(PathBuf::from("/tmp/cfg.sock")));
        std::env::remove_var("SSH_AUTH_SOCK");
    }
}
