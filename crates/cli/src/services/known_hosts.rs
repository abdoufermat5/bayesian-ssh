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
    Mismatch {
        stored_fp: String,
        remote_fp: String,
    },
    /// The presented key is listed under a `@revoked` marker. Must never be
    /// accepted, regardless of the host-key-checking policy.
    Revoked { remote_fp: String },
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
    /// OpenSSH semantics: entries for non-default ports are stored as
    /// `[host]:port`, and only that canonical form is matched.
    fn matches(&self, hostname: &str, port: u16) -> bool {
        let canonical = canonical_hostport(hostname, port);
        match self {
            HostPattern::Plain(p) => glob_match(p, &canonical),
            HostPattern::Hashed { salt, hash } => {
                hmac_sha1_matches(salt, hash, canonical.as_bytes())
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
        text.starts_with(prefix)
            && text.ends_with(suffix)
            && text.len() >= prefix.len() + suffix.len()
    } else {
        pattern == text
    }
}

fn parse_entry(line: &str) -> Option<KnownEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    // OpenSSH separates fields with any run of spaces/tabs.
    let mut parts = line.split_whitespace();

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
    let key_bytes = B64.decode(key_b64).ok()?;

    let patterns = host_field
        .split(',')
        .filter_map(|p| parse_host_pattern(p.trim()))
        .collect::<Vec<_>>();

    if patterns.is_empty() {
        return None;
    }

    Some(KnownEntry {
        marker,
        patterns,
        key_type,
        key_bytes,
    })
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
/// `key_type` must be the key *blob* algorithm as written in known_hosts
/// (e.g. `ssh-rsa`), not a signature algorithm such as `rsa-sha2-512`; use
/// [`blob_key_type`] to derive it from the wire-format key.
///
/// Returns:
/// - `CheckResult::Revoked`     — the key is listed under `@revoked`
/// - `CheckResult::KnownGood`   — an entry for this host/type has this key
/// - `CheckResult::Mismatch`    — entries for this host/type exist, none match (TOFU violation)
/// - `CheckResult::Unknown`     — no matching host/type entry at all
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

    // Scan the whole file: a host may legitimately have several entries for
    // the same key type (rotation, hashed + plain), and a `@revoked` line
    // later in the file must still win over an earlier match.
    let mut known_good = false;
    let mut stored_fp: Option<String> = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Some(entry) = parse_entry(&line) else {
            continue;
        };
        if !entry.patterns.iter().any(|p| p.matches(hostname, port)) {
            continue;
        }

        match entry.marker.as_str() {
            "@revoked" => {
                if entry.key_bytes == key_bytes {
                    return Ok(CheckResult::Revoked { remote_fp });
                }
            }
            // `@cert-authority` lines hold CA keys, not host keys; host
            // certificates are not supported, so they neither vouch for nor
            // contradict a plain host key.
            "" if entry.key_type == key_type => {
                if entry.key_bytes == key_bytes {
                    known_good = true;
                } else if stored_fp.is_none() {
                    stored_fp = Some(fingerprint_sha256(&entry.key_bytes));
                }
            }
            _ => {}
        }
    }

    Ok(match (known_good, stored_fp) {
        (true, _) => CheckResult::KnownGood,
        (false, Some(stored_fp)) => CheckResult::Mismatch {
            stored_fp,
            remote_fp,
        },
        (false, None) => CheckResult::Unknown,
    })
}

/// Extract the key algorithm name (first SSH `string`) from a wire-format
/// public key blob, e.g. `ssh-rsa` / `ssh-ed25519` / `ecdsa-sha2-nistp256`.
pub fn blob_key_type(key_bytes: &[u8]) -> Option<&str> {
    let len_bytes: [u8; 4] = key_bytes.get(..4)?.try_into().ok()?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let name = key_bytes.get(4..4usize.checked_add(len)?)?;
    std::str::from_utf8(name).ok()
}

/// Append a new plain-text entry to `path` (TOFU accept-new).
pub fn append(
    path: &Path,
    hostname: &str,
    port: u16,
    key_type: &str,
    key_bytes: &[u8],
) -> Result<(), KnownHostsError> {
    // Ensure parent directory exists (0700, as OpenSSH expects for ~/.ssh)
    if let Some(parent) = path.parent() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
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

    #[test]
    fn revoked_key_is_reported_even_after_good_entry() {
        let f = NamedTempFile::new().unwrap();
        let key = dummy_key(0x07);
        append(f.path(), "rev.example", 22, "ssh-ed25519", &key).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(f.path()).unwrap();
        writeln!(file, "@revoked * ssh-ed25519 {}", B64.encode(&key)).unwrap();
        let r = check(f.path(), "rev.example", 22, "ssh-ed25519", &key).unwrap();
        assert!(matches!(r, CheckResult::Revoked { .. }));
    }

    #[test]
    fn any_matching_entry_wins_over_stale_one() {
        let f = NamedTempFile::new().unwrap();
        append(f.path(), "rot.example", 22, "ssh-ed25519", &dummy_key(0x01)).unwrap();
        append(f.path(), "rot.example", 22, "ssh-ed25519", &dummy_key(0x02)).unwrap();
        let r = check(f.path(), "rot.example", 22, "ssh-ed25519", &dummy_key(0x02)).unwrap();
        assert_eq!(r, CheckResult::KnownGood);
    }

    #[test]
    fn cert_authority_line_is_not_a_host_key() {
        let f = NamedTempFile::new().unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(f.path()).unwrap();
        writeln!(
            file,
            "@cert-authority ca.example ssh-ed25519 {}",
            B64.encode(dummy_key(9))
        )
        .unwrap();
        let r = check(f.path(), "ca.example", 22, "ssh-ed25519", &dummy_key(1)).unwrap();
        assert_eq!(r, CheckResult::Unknown);
    }

    #[test]
    fn default_port_entry_does_not_match_other_port() {
        let f = NamedTempFile::new().unwrap();
        append(f.path(), "multi.example", 22, "ssh-ed25519", &dummy_key(1)).unwrap();
        let r = check(
            f.path(),
            "multi.example",
            2222,
            "ssh-ed25519",
            &dummy_key(2),
        )
        .unwrap();
        assert_eq!(r, CheckResult::Unknown);
    }

    #[test]
    fn blob_key_type_reads_wire_name() {
        let mut blob = Vec::new();
        blob.extend_from_slice(&7u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-rsa");
        blob.extend_from_slice(&[0, 0, 0, 1, 0x23]);
        assert_eq!(blob_key_type(&blob), Some("ssh-rsa"));
        assert_eq!(blob_key_type(&[0, 0, 0, 9, b'x']), None);
        assert_eq!(blob_key_type(&[0xff, 0xff, 0xff, 0xff]), None);
        assert_eq!(blob_key_type(&[]), None);
    }
}
