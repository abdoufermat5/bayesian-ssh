//! Known-hosts parser and verifier.
//!
//! Supports both plain (`hostname key-type base64-key`) and hashed
//! (`|1|base64-salt|base64-hash`) entries as written by OpenSSH.

#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use sha1::Digest;
use sha2::Sha256;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

type HmacSha1 = Hmac<sha1::Sha1>;

#[derive(Debug, Error)]
pub enum KnownHostsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("host key mismatch for {host}: remote key has fingerprint {remote_fp}, stored is {stored_fp}")]
    KeyMismatch {
        host: String,
        remote_fp: String,
        stored_fp: String,
    },
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
}

/// Result of checking a host key against known_hosts.
#[derive(Debug, PartialEq)]
pub enum CheckResult {
    /// Host is known and key matches.
    KnownGood,
    /// Host is not in the file (new host).
    Unknown,
    /// Host is known but the key has changed — likely MITM.
    Mismatch { stored_fp: String, remote_fp: String },
}

/// Parsed entry from a known_hosts file.
#[derive(Debug, Clone)]
struct KnownEntry {
    /// Raw marker field (e.g. `@revoked`, `@cert-authority`) — empty if absent.
    marker: String,
    /// Canonical host patterns for matching (already decoded if hashed).
    patterns: Vec<HostPattern>,
    key_type: String,
    key_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
enum HostPattern {
    Plain(String),
    Hashed { salt: Vec<u8>, hash: Vec<u8> },
}

impl HostPattern {
    fn matches(&self, hostname: &str, port: u16) -> bool {
        let canonical = canonical_hostport(hostname, port);
        match self {
            HostPattern::Plain(p) => {
                p == &canonical
                    || p == hostname
                    || glob_match(p, &canonical)
                    || glob_match(p, hostname)
            }
            HostPattern::Hashed { salt, hash } => {
                hmac_sha1_matches(salt, hash, canonical.as_bytes())
                    || hmac_sha1_matches(salt, hash, hostname.as_bytes())
            }
        }
    }
}

fn canonical_hostport(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn hmac_sha1_matches(salt: &[u8], expected_hash: &[u8], data: &[u8]) -> bool {
    let mut mac = HmacSha1::new_from_slice(salt).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().as_slice() == expected_hash
}

/// Minimal glob: only `*` wildcard (no `?`, no charset).
fn glob_match(pattern: &str, text: &str) -> bool {
    if let Some(star) = pattern.find('*') {
        let prefix = &pattern[..star];
        let suffix = &pattern[star + 1..];
        text.starts_with(prefix) && text.ends_with(suffix) && text.len() >= prefix.len() + suffix.len()
    } else {
        pattern == text
    }
}

fn parse_entry(line: &str) -> Option<KnownEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let mut parts = line.splitn(4, ' ');

    let (marker, host_field) = {
        let first = parts.next()?;
        if first.starts_with('@') {
            (first.to_string(), parts.next()?.to_string())
        } else {
            (String::new(), first.to_string())
        }
    };

    let key_type = parts.next()?.to_string();
    let key_b64 = parts.next()?;
    let key_bytes = B64.decode(key_b64.trim()).ok()?;

    let patterns = host_field
        .split(',')
        .filter_map(|p| parse_host_pattern(p.trim()))
        .collect::<Vec<_>>();

    if patterns.is_empty() {
        return None;
    }

    Some(KnownEntry { marker, patterns, key_type, key_bytes })
}

fn parse_host_pattern(pattern: &str) -> Option<HostPattern> {
    if let Some(rest) = pattern.strip_prefix("|1|") {
        // |1|base64-salt|base64-hash
        let mut it = rest.splitn(2, '|');
        let salt = B64.decode(it.next()?).ok()?;
        let hash = B64.decode(it.next()?).ok()?;
        Some(HostPattern::Hashed { salt, hash })
    } else {
        // strip negation prefix `!` — we don't support deny patterns but skip gracefully
        if pattern.starts_with('!') {
            None
        } else {
            Some(HostPattern::Plain(pattern.to_string()))
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────────

/// Return the default path: `~/.ssh/known_hosts`.
pub fn default_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".ssh")
        .join("known_hosts")
}

/// Check whether `(hostname, port, key_type, key_bytes)` matches what is stored.
///
/// Returns:
/// - `CheckResult::KnownGood`   — entry found and key matches
/// - `CheckResult::Unknown`     — no matching hostname entry at all
/// - `CheckResult::Mismatch`    — entry found, but key differs (TOFU violation)
pub fn check(
    path: &Path,
    hostname: &str,
    port: u16,
    key_type: &str,
    key_bytes: &[u8],
) -> Result<CheckResult, KnownHostsError> {
    let remote_fp = fingerprint_sha256(key_bytes);

    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CheckResult::Unknown),
        Err(e) => return Err(KnownHostsError::Io(e)),
    };

    for line in BufReader::new(file).lines() {
        let line = line?;
        let Some(entry) = parse_entry(&line) else { continue };

        if entry.marker == "@revoked" {
            continue;
        }
        if entry.key_type != key_type {
            continue;
        }
        let matches_host = entry.patterns.iter().any(|p| p.matches(hostname, port));
        if !matches_host {
            continue;
        }

        // Host + key_type matched — check the key bytes
        if entry.key_bytes == key_bytes {
            return Ok(CheckResult::KnownGood);
        } else {
            let stored_fp = fingerprint_sha256(&entry.key_bytes);
            return Ok(CheckResult::Mismatch { stored_fp, remote_fp });
        }
    }

    Ok(CheckResult::Unknown)
}

/// Append a new plain-text entry to `path` (TOFU accept-new).
pub fn append(
    path: &Path,
    hostname: &str,
    port: u16,
    key_type: &str,
    key_bytes: &[u8],
) -> Result<(), KnownHostsError> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    let host_field = canonical_hostport(hostname, port);
    let key_b64 = B64.encode(key_bytes);
    writeln!(file, "{host_field} {key_type} {key_b64}")?;
    Ok(())
}

/// SHA-256 fingerprint in the `SHA256:<base64>` format shown by OpenSSH.
pub fn fingerprint_sha256(key_bytes: &[u8]) -> String {
    let digest = Sha256::digest(key_bytes);
    let encoded = B64.encode(digest);
    // OpenSSH omits trailing `=`
    format!("SHA256:{}", encoded.trim_end_matches('='))
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn dummy_key(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    #[test]
    fn append_and_find_plain_entry() {
        let f = NamedTempFile::new().unwrap();
        let key = dummy_key(0xAB);
        append(f.path(), "example.com", 22, "ssh-ed25519", &key).unwrap();
        let result = check(f.path(), "example.com", 22, "ssh-ed25519", &key).unwrap();
        assert_eq!(result, CheckResult::KnownGood);
    }

    #[test]
    fn unknown_host_returns_unknown() {
        let f = NamedTempFile::new().unwrap();
        let key = dummy_key(0x01);
        let result = check(f.path(), "nowhere.example", 22, "ssh-ed25519", &key).unwrap();
        assert_eq!(result, CheckResult::Unknown);
    }

    #[test]
    fn mismatch_detected() {
        let f = NamedTempFile::new().unwrap();
        let stored = dummy_key(0x01);
        let remote = dummy_key(0x02);
        append(f.path(), "host.example", 22, "ssh-ed25519", &stored).unwrap();
        let result = check(f.path(), "host.example", 22, "ssh-ed25519", &remote).unwrap();
        assert!(matches!(result, CheckResult::Mismatch { .. }));
    }

    #[test]
    fn non_standard_port_canonical_form() {
        let f = NamedTempFile::new().unwrap();
        let key = dummy_key(0x42);
        append(f.path(), "srv.example", 2222, "ssh-ed25519", &key).unwrap();
        // Standard port should NOT find the entry stored under port 2222
        let r1 = check(f.path(), "srv.example", 22, "ssh-ed25519", &key).unwrap();
        assert_eq!(r1, CheckResult::Unknown);
        // Correct port should match
        let r2 = check(f.path(), "srv.example", 2222, "ssh-ed25519", &key).unwrap();
        assert_eq!(r2, CheckResult::KnownGood);
    }

    #[test]
    fn glob_star_matches_subdomain() {
        assert!(glob_match("*.example.com", "foo.example.com"));
        assert!(!glob_match("*.example.com", "example.com"));
    }

    #[test]
    fn fingerprint_format() {
        let key = dummy_key(0xFF);
        let fp = fingerprint_sha256(&key);
        assert!(fp.starts_with("SHA256:"));
        assert!(!fp.ends_with('='));
    }
}
