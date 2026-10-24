//! Local signed hash-chained AI audit log (AuditSink) and Teams forwarding hook.
//!
//! Owner: W6. One JSONL file, `<ai_dir>/audit/events.jsonl`; each line is
//! `{seq, ts, event, prev_hash, hash, sig}`. `hash` is SHA-256 over
//! `seq \n ts \n canonical event JSON \n prev_hash`, `sig` is an Ed25519
//! signature of the hash hex by the local audit key (`audit/device.key`).

use super::types::{AiEvent, AuditSink};
use crate::config::AppConfig;
use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// `prev_hash` of the first entry.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One line of the log, event kept as raw JSON so the hash can be re-checked exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawEntry {
    pub seq: u64,
    /// RFC 3339 UTC.
    pub ts: String,
    pub event: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
    pub sig: String,
}

/// One log entry with the event decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub seq: u64,
    pub ts: String,
    pub event: AiEvent,
    pub hash: String,
}

/// Result of `verify`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub entries: u64,
    pub ok: bool,
    /// Sequence number of the first bad entry.
    pub first_bad: Option<u64>,
    pub error: Option<String>,
}

pub fn audit_dir(config: &AppConfig) -> PathBuf {
    super::ai_dir(config).join("audit")
}

pub fn events_path(config: &AppConfig) -> PathBuf {
    audit_dir(config).join("events.jsonl")
}

fn key_path(dir: &Path) -> PathBuf {
    dir.join("device.key")
}

fn pub_path(dir: &Path) -> PathBuf {
    dir.join("device.pub")
}

fn entry_hash(seq: u64, ts: &str, event: &serde_json::Value, prev_hash: &str) -> String {
    let canon = format!("{seq}\n{ts}\n{event}\n{prev_hash}");
    hex(&Sha256::digest(canon.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        anyhow::bail!("bad hex");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| anyhow::anyhow!("bad hex: {e}")))
        .collect()
}

/// Create a directory (and parents) with 0700 on unix.
fn ensure_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    crate::config::enforce_secure_dir(dir);
    Ok(())
}

fn open_private(path: &Path, append: bool) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true).create(true);
    if append {
        o.append(true);
    } else {
        o.write(true);
    }
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let f = o
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    crate::config::enforce_secure_file(path);
    Ok(f)
}

/// Load the audit key, creating it (0600) on first use. Called with the log lock held.
fn load_or_create_key(dir: &Path) -> Result<SigningKey> {
    let kp = key_path(dir);
    if kp.exists() {
        let text = std::fs::read_to_string(&kp).context("read audit key")?;
        let bytes = unhex(text.trim())?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("audit key has the wrong length"))?;
        return Ok(SigningKey::from_bytes(&arr));
    }
    let mut seed = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut seed);
    let key = SigningKey::from_bytes(&seed);
    let mut f = open_private(&kp, false)?;
    f.write_all(hex(&seed).as_bytes())?;
    f.flush()?;
    let pp = pub_path(dir);
    std::fs::write(&pp, hex(key.verifying_key().as_bytes()))?;
    Ok(key)
}

/// Last non-empty line of a file, read from the end.
fn last_line(f: &mut std::fs::File) -> Result<Option<String>> {
    let len = f.metadata()?.len();
    let mut chunk = 8192u64;
    loop {
        let start = len.saturating_sub(chunk);
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        f.take(len - start).read_to_end(&mut buf)?;
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            return Ok(None);
        }
        if let Some(i) = trimmed.rfind('\n') {
            return Ok(Some(trimmed[i + 1..].to_string()));
        }
        if start == 0 {
            return Ok(Some(trimmed.to_string()));
        }
        chunk *= 4;
    }
}

/// Append-only, hash-chained, Ed25519-signed JSONL log in `<ai_dir>/audit/`.
pub struct LocalAudit {
    config: AppConfig,
    dir: PathBuf,
}

impl LocalAudit {
    pub fn open(config: &AppConfig) -> anyhow::Result<Self> {
        let dir = audit_dir(config);
        ensure_dir(dir.parent().unwrap_or(&dir))?;
        ensure_dir(&dir)?;
        Ok(Self {
            config: config.clone(),
            dir,
        })
    }

    /// Append one event; returns the new entry's sequence number.
    pub fn append(&self, event: &AiEvent) -> Result<u64> {
        let value = serde_json::to_value(event)?;
        let mut f = open_private(&self.dir.join("events.jsonl"), true)?;
        // Several `bssh ai mcp` processes share the file: hold the lock for the whole append.
        f.lock().context("lock audit log")?;
        let res = self.append_locked(&mut f, value);
        let _ = f.unlock();
        res
    }

    fn append_locked(&self, f: &mut std::fs::File, event: serde_json::Value) -> Result<u64> {
        let key = load_or_create_key(&self.dir)?;
        let (seq, prev_hash) = match last_line(f)? {
            Some(line) => {
                let last: RawEntry =
                    serde_json::from_str(&line).context("last audit line is not valid")?;
                (last.seq + 1, last.hash)
            }
            None => (1, GENESIS_HASH.to_string()),
        };
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let hash = entry_hash(seq, &ts, &event, &prev_hash);
        let sig = hex(&key.sign(hash.as_bytes()).to_bytes());
        let entry = RawEntry {
            seq,
            ts,
            event,
            prev_hash,
            hash,
            sig,
        };
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.flush()?;
        Ok(seq)
    }
}

impl AuditSink for LocalAudit {
    fn record(&self, event: &AiEvent) -> anyhow::Result<()> {
        self.append(event)?;
        // Local append is the source of truth; a forwarding failure must not lose the event.
        if let Err(e) = super::teams::forward_event(&self.config, event) {
            tracing::warn!("forwarding AI audit event to Teams failed: {e}");
        }
        Ok(())
    }
}

fn read_raw(path: &Path) -> Result<Vec<RawEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).context("read audit log")?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            serde_json::from_str(l).with_context(|| format!("audit line {} is not valid", i + 1))
        })
        .collect()
}

/// All entries in order. Lines whose event can't be decoded are skipped with a warning.
pub fn read_entries(config: &AppConfig) -> Result<Vec<AuditEntry>> {
    let mut out = Vec::new();
    for raw in read_raw(&events_path(config))? {
        match serde_json::from_value::<AiEvent>(raw.event.clone()) {
            Ok(event) => out.push(AuditEntry {
                seq: raw.seq,
                ts: raw.ts,
                event,
                hash: raw.hash,
            }),
            Err(e) => tracing::warn!("audit entry {} has an unknown event: {e}", raw.seq),
        }
    }
    Ok(out)
}

fn bad(entries: u64, seq: u64, msg: &str) -> VerifyReport {
    VerifyReport {
        entries,
        ok: false,
        first_bad: Some(seq),
        error: Some(msg.to_string()),
    }
}

/// Check sequence, hash chain and signatures of the whole log.
pub fn verify(config: &AppConfig) -> Result<VerifyReport> {
    let dir = audit_dir(config);
    let raw = match read_raw(&events_path(config)) {
        Ok(r) => r,
        Err(e) => {
            return Ok(VerifyReport {
                entries: 0,
                ok: false,
                first_bad: None,
                error: Some(format!("{e:#}")),
            })
        }
    };
    let total = raw.len() as u64;
    if raw.is_empty() {
        return Ok(VerifyReport {
            entries: 0,
            ok: true,
            first_bad: None,
            error: None,
        });
    }
    let vk = match std::fs::read_to_string(pub_path(&dir)) {
        Ok(t) => {
            let arr: [u8; 32] = unhex(t.trim())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("audit public key has the wrong length"))?;
            VerifyingKey::from_bytes(&arr)?
        }
        Err(_) => {
            let t =
                std::fs::read_to_string(key_path(&dir)).context("no audit key to verify with")?;
            let arr: [u8; 32] = unhex(t.trim())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("audit key has the wrong length"))?;
            SigningKey::from_bytes(&arr).verifying_key()
        }
    };
    let mut prev = GENESIS_HASH.to_string();
    for (i, e) in raw.iter().enumerate() {
        let want_seq = i as u64 + 1;
        if e.seq != want_seq {
            return Ok(bad(total, e.seq, "sequence gap or reorder"));
        }
        if e.prev_hash != prev {
            return Ok(bad(
                total,
                e.seq,
                "prev_hash does not match the previous entry",
            ));
        }
        if entry_hash(e.seq, &e.ts, &e.event, &e.prev_hash) != e.hash {
            return Ok(bad(total, e.seq, "hash mismatch (entry was modified)"));
        }
        let sig_ok = unhex(&e.sig)
            .ok()
            .and_then(|b| <[u8; 64]>::try_from(b).ok())
            .map(|b| {
                vk.verify(e.hash.as_bytes(), &Signature::from_bytes(&b))
                    .is_ok()
            })
            .unwrap_or(false);
        if !sig_ok {
            return Ok(bad(total, e.seq, "bad signature"));
        }
        prev = e.hash.clone();
    }
    Ok(VerifyReport {
        entries: total,
        ok: true,
        first_bad: None,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    pub(crate) fn cfg(dir: &Path) -> AppConfig {
        AppConfig {
            database_path: dir.join("env").join("bssh.db"),
            ..Default::default()
        }
    }

    fn started(n: usize) -> AiEvent {
        AiEvent::RunStarted {
            run_id: format!("run-{n}"),
            client: "test".into(),
            on_behalf_of: "alice".into(),
            policy: "default".into(),
            policy_hash: "h".into(),
        }
    }

    #[test]
    fn append_and_verify_chain() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        let a = LocalAudit::open(&c).unwrap();
        for i in 0..5 {
            a.record(&started(i)).unwrap();
        }
        let r = verify(&c).unwrap();
        assert!(r.ok, "{r:?}");
        assert_eq!(r.entries, 5);
        assert_eq!(read_entries(&c).unwrap().len(), 5);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(m(events_path(&c)), 0o600);
            assert_eq!(m(audit_dir(&c).join("device.key")), 0o600);
        }
    }

    #[test]
    fn empty_log_verifies() {
        let d = tempfile::tempdir().unwrap();
        let r = verify(&cfg(d.path())).unwrap();
        assert!(r.ok && r.entries == 0);
    }

    #[test]
    fn tamper_is_detected() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        let a = LocalAudit::open(&c).unwrap();
        for i in 0..3 {
            a.record(&started(i)).unwrap();
        }
        let p = events_path(&c);
        let text = std::fs::read_to_string(&p).unwrap();
        // Modify an event.
        std::fs::write(&p, text.replace("run-1", "run-X")).unwrap();
        let r = verify(&c).unwrap();
        assert!(!r.ok);
        assert_eq!(r.first_bad, Some(2));
        // Delete the middle line: gap.
        let lines: Vec<&str> = text.lines().collect();
        std::fs::write(&p, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
        let r = verify(&c).unwrap();
        assert!(!r.ok);
        assert_eq!(r.first_bad, Some(3));
        // Restore, then forge a signature.
        std::fs::write(&p, &text).unwrap();
        assert!(verify(&c).unwrap().ok);
        let mut raw: Vec<RawEntry> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        raw[2].sig = "00".repeat(64);
        let forged: String = raw
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        std::fs::write(&p, forged).unwrap();
        let r = verify(&c).unwrap();
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some("bad signature"));
    }

    #[test]
    fn concurrent_appends_keep_chain() {
        let d = tempfile::tempdir().unwrap();
        let c = Arc::new(cfg(d.path()));
        let mut hs = Vec::new();
        for t in 0..4 {
            let c = c.clone();
            hs.push(std::thread::spawn(move || {
                // Separate handles, like separate processes.
                let a = LocalAudit::open(&c).unwrap();
                for i in 0..10 {
                    a.record(&started(t * 100 + i)).unwrap();
                }
            }));
        }
        for h in hs {
            h.join().unwrap();
        }
        let r = verify(&c).unwrap();
        assert!(r.ok, "{r:?}");
        assert_eq!(r.entries, 40);
    }
}
