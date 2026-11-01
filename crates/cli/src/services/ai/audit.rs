//! Local signed hash-chained AI audit log (AuditSink) and Teams forwarding hook.
//!
//! One JSONL file, `<ai_dir>/audit/events.jsonl`; each line is
//! `{seq, ts, event, prev_hash, hash, sig}`. `hash` is SHA-256 over
//! `seq \n ts \n canonical event JSON \n prev_hash`, `sig` is an Ed25519
//! signature of the hash hex by the local audit key (`audit/device.key`).
//!
//! Pruning (`super::prune`) keeps the chain verifiable without the pruned events:
//!
//! * a pruned entry stays as a **stub**: same `seq, ts, prev_hash, hash, sig`, `event: null`
//!   and `pruned: true`. The signature covers the hash, so it still verifies; only the hash
//!   itself can no longer be recomputed;
//! * the leading run of stubs is dropped from the file and replaced by `audit/anchor.json`,
//!   `{seq, hash, pruned_at, sig}`, signed over `anchor\n{seq}\n{hash}`. Verification starts
//!   after the anchor;
//! * every rewrite happens under the same exclusive lock as [`LocalAudit::append`], through a
//!   temp file and a rename. The rename swaps the inode, so a writer that was waiting for the
//!   lock re-checks that its handle is still the file at the path and reopens it if not.

use super::types::{AiEvent, AuditSink};
use crate::config::AppConfig;
use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
    /// `null` in a stub.
    pub event: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
    pub sig: String,
    /// A stub: the event was pruned, the chain links and the signature are kept.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pruned: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Where the chain resumes after pruned entries were dropped from the front of the log.
/// `sig` signs `anchor\n{seq}\n{hash}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// Sequence number of the last dropped entry.
    pub seq: u64,
    /// Its hash: the `prev_hash` of the first entry left in the log.
    pub hash: String,
    /// RFC 3339 UTC.
    pub pruned_at: String,
    pub sig: String,
}

/// One line of the log with its exact text.
#[derive(Debug, Clone)]
pub(super) struct LogLine {
    pub text: String,
    pub entry: RawEntry,
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

/// `<audit dir>/anchor.json`.
pub fn anchor_path(config: &AppConfig) -> PathBuf {
    audit_dir(config).join("anchor.json")
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

/// How [`open_private`] positions writes.
#[derive(Clone, Copy)]
enum WriteMode {
    Append,
    Overwrite,
}

fn open_private(path: &Path, mode: WriteMode) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true).create(true);
    match mode {
        WriteMode::Append => o.append(true),
        WriteMode::Overwrite => o.write(true),
    };
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
    let mut f = open_private(&kp, WriteMode::Overwrite)?;
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

fn anchor_message(seq: u64, hash: &str) -> String {
    format!("anchor\n{seq}\n{hash}")
}

fn read_anchor(dir: &Path) -> Result<Option<Anchor>> {
    match std::fs::read_to_string(dir.join("anchor.json")) {
        Ok(t) => Ok(Some(
            serde_json::from_str(&t).context("audit anchor is not valid")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("read audit anchor"),
    }
}

/// The signing key, which must already exist: pruning never invents a new one.
fn load_key(dir: &Path) -> Result<SigningKey> {
    anyhow::ensure!(
        key_path(dir).exists(),
        "the audit key is missing, so the log cannot be re-anchored"
    );
    load_or_create_key(dir)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LockKind {
    Exclusive,
    Shared,
}

/// Whether `f` is still the file at `path`. A prune or wipe replaces the file, and a handle
/// opened before that points at an unlinked inode whose writes nobody would ever read.
#[cfg(unix)]
fn is_current(f: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (f.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Elsewhere the file cannot be replaced while a handle is open, so the handle is current.
#[cfg(not(unix))]
fn is_current(_f: &std::fs::File, _path: &Path) -> bool {
    true
}

/// Open the log and take the lock, retrying while the file is replaced under us.
/// Exclusive creates the file; Shared returns `None` when there is none.
fn lock_log(path: &Path, kind: LockKind) -> Result<Option<std::fs::File>> {
    for _ in 0..1000 {
        let f = match kind {
            LockKind::Exclusive => open_private(path, WriteMode::Append)?,
            LockKind::Shared => match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
            },
        };
        match kind {
            LockKind::Exclusive => f.lock(),
            LockKind::Shared => f.lock_shared(),
        }
        .context("lock audit log")?;
        if is_current(&f, path) {
            return Ok(Some(f));
        }
        let _ = f.unlock();
    }
    anyhow::bail!("the audit log keeps being replaced")
}

fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Write `<dir>/<name>` through a 0600 temp file in `dir`: fsync, rename, fsync the directory.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let res = (|| -> Result<()> {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, dir.join(name)).with_context(|| format!("replace {name}"))?;
        sync_dir(dir);
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

fn parse_lines(text: &str) -> Result<Vec<LogLine>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            Ok(LogLine {
                text: l.to_string(),
                entry: serde_json::from_str(l)
                    .with_context(|| format!("audit line {} is not valid", i + 1))?,
            })
        })
        .collect()
}

/// The log and its anchor as one consistent snapshot, read under a shared lock.
fn snapshot(config: &AppConfig) -> Result<(Vec<LogLine>, Option<Anchor>)> {
    let dir = audit_dir(config);
    let Some(mut f) = lock_log(&events_path(config), LockKind::Shared)? else {
        return Ok((Vec::new(), read_anchor(&dir)?));
    };
    let mut text = String::new();
    let read = f.read_to_string(&mut text).context("read audit log");
    // Anchor second: a prune writes it before the log, so it is never older than the text.
    let anchor = read_anchor(&dir);
    let _ = f.unlock();
    read?;
    Ok((parse_lines(&text)?, anchor?))
}

/// Every line of the log, stubs included, from a consistent snapshot.
pub(super) fn read_lines(config: &AppConfig) -> Result<Vec<LogLine>> {
    Ok(snapshot(config)?.0)
}

/// Counts of one [`LockedLog::rewrite`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Rewritten {
    /// Entries turned into stubs by this call.
    pub stubbed: u64,
    /// Leading stubs folded into the anchor.
    pub dropped: u64,
}

/// The log held under the exclusive append lock. Everything that rewrites the file goes
/// through here; the lock is released on drop.
pub(super) struct LockedLog {
    file: std::fs::File,
    dir: PathBuf,
}

impl LockedLog {
    /// Wait for the lock (appenders hold it for one line).
    pub(super) fn lock(config: &AppConfig) -> Result<Self> {
        let dir = audit_dir(config);
        ensure_dir(dir.parent().unwrap_or(&dir))?;
        ensure_dir(&dir)?;
        let file = lock_log(&events_path(config), LockKind::Exclusive)?
            .context("audit log vanished while locking")?;
        Ok(Self { file, dir })
    }

    pub(super) fn lines(&mut self) -> Result<Vec<LogLine>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        self.file
            .read_to_string(&mut text)
            .context("read audit log")?;
        parse_lines(&text)
    }

    /// Turn the entries in `stub_seqs` into stubs and fold the leading stubs into the anchor.
    /// The anchor is written first, then the log: if the process dies in between, the log
    /// still starts before the anchor and `verify` skips what the anchor already covers.
    pub(super) fn rewrite(&mut self, stub_seqs: &HashSet<u64>) -> Result<Rewritten> {
        let lines = self.lines()?;
        let mut out = String::new();
        let mut done = Rewritten::default();
        let mut leading = true;
        let mut last_dropped: Option<&RawEntry> = None;
        for l in &lines {
            let e = &l.entry;
            let stub = e.pruned || stub_seqs.contains(&e.seq);
            if leading && stub {
                done.dropped += 1;
                if !e.pruned {
                    done.stubbed += 1;
                }
                last_dropped = Some(e);
                continue;
            }
            leading = false;
            if stub && !e.pruned {
                done.stubbed += 1;
                let s = RawEntry {
                    event: serde_json::Value::Null,
                    pruned: true,
                    ..e.clone()
                };
                out.push_str(&serde_json::to_string(&s)?);
            } else {
                out.push_str(&l.text);
            }
            out.push('\n');
        }
        if done.stubbed == 0 {
            return Ok(done);
        }
        if let Some(last) = last_dropped {
            let key = load_key(&self.dir)?;
            let sig = hex(&key
                .sign(anchor_message(last.seq, &last.hash).as_bytes())
                .to_bytes());
            let anchor = Anchor {
                seq: last.seq,
                hash: last.hash.clone(),
                pruned_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                sig,
            };
            write_atomic(
                &self.dir,
                "anchor.json",
                serde_json::to_string_pretty(&anchor)?.as_bytes(),
            )?;
        }
        write_atomic(&self.dir, "events.jsonl", out.as_bytes())?;
        Ok(done)
    }

    /// Remove the log and the anchor; the next append starts from genesis. The key stays.
    /// The anchor goes first. While `events.jsonl` still exists, writers wait on its lock;
    /// once it is gone a new writer starts a fresh file, and it must not find an old anchor
    /// to continue from. A crash in between leaves a log that fails `verify` until the
    /// wipe is run again.
    pub(super) fn wipe(&mut self) -> Result<()> {
        for name in ["anchor.json", "events.jsonl"] {
            match std::fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("remove {name}")),
            }
        }
        sync_dir(&self.dir);
        Ok(())
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
        // Several `bssh ai mcp` processes share the file: hold the lock for the whole append.
        // A prune may have replaced the file while we waited; `lock_log` reopens it then.
        let mut f = lock_log(&self.dir.join("events.jsonl"), LockKind::Exclusive)?
            .context("audit log vanished while locking")?;
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
            // Empty log: continue after the anchor of a pruned one, else start the chain.
            None => match read_anchor(&self.dir)? {
                Some(a) => (a.seq + 1, a.hash),
                None => (1, GENESIS_HASH.to_string()),
            },
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
            pruned: false,
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

/// All entries in order. Stubs are skipped silently; lines whose event can't be decoded are
/// skipped with a warning.
pub fn read_entries(config: &AppConfig) -> Result<Vec<AuditEntry>> {
    let mut out = Vec::new();
    for line in read_lines(config)? {
        let raw = line.entry;
        if raw.pruned {
            continue;
        }
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

fn sig_ok(vk: &VerifyingKey, msg: &str, sig: &str) -> bool {
    unhex(sig)
        .ok()
        .and_then(|b| <[u8; 64]>::try_from(b).ok())
        .map(|b| {
            vk.verify(msg.as_bytes(), &Signature::from_bytes(&b))
                .is_ok()
        })
        .unwrap_or(false)
}

fn verifying_key(dir: &Path) -> Result<VerifyingKey> {
    match std::fs::read_to_string(pub_path(dir)) {
        Ok(t) => {
            let arr: [u8; 32] = unhex(t.trim())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("audit public key has the wrong length"))?;
            Ok(VerifyingKey::from_bytes(&arr)?)
        }
        Err(_) => {
            let t =
                std::fs::read_to_string(key_path(dir)).context("no audit key to verify with")?;
            let arr: [u8; 32] = unhex(t.trim())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("audit key has the wrong length"))?;
            Ok(SigningKey::from_bytes(&arr).verifying_key())
        }
    }
}

/// Check sequence, hash chain and signatures of the whole log. The chain starts at the
/// anchor when there is one. A stub is checked for its position, its link to the previous
/// entry and its signature; its hash can't be recomputed because the event is gone.
pub fn verify(config: &AppConfig) -> Result<VerifyReport> {
    let dir = audit_dir(config);
    let (lines, anchor) = match snapshot(config) {
        Ok(s) => s,
        Err(e) => {
            return Ok(VerifyReport {
                entries: 0,
                ok: false,
                first_bad: None,
                error: Some(format!("{e:#}")),
            });
        }
    };
    let total = lines.len() as u64;
    if lines.is_empty() && anchor.is_none() {
        return Ok(VerifyReport {
            entries: 0,
            ok: true,
            first_bad: None,
            error: None,
        });
    }
    let vk = verifying_key(&dir)?;
    let (mut prev, mut want_seq) = match &anchor {
        Some(a) => {
            if !sig_ok(&vk, &anchor_message(a.seq, &a.hash), &a.sig) {
                return Ok(bad(total, a.seq, "bad anchor signature"));
            }
            (a.hash.clone(), a.seq + 1)
        }
        None => (GENESIS_HASH.to_string(), 1),
    };
    for l in &lines {
        let e = &l.entry;
        if let Some(a) = &anchor
            && e.seq <= a.seq
        {
            // Left over from a prune that stopped before rewriting the log: the anchor
            // already covers it, as long as it is the same chain.
            if e.seq == a.seq && e.hash != a.hash {
                return Ok(bad(total, e.seq, "entry does not match the anchor"));
            }
            continue;
        }
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
        if e.pruned {
            if !e.event.is_null() {
                return Ok(bad(total, e.seq, "pruned entry still carries an event"));
            }
        } else if entry_hash(e.seq, &e.ts, &e.event, &e.prev_hash) != e.hash {
            return Ok(bad(total, e.seq, "hash mismatch (entry was modified)"));
        }
        if !sig_ok(&vk, &e.hash, &e.sig) {
            return Ok(bad(total, e.seq, "bad signature"));
        }
        prev = e.hash.clone();
        want_seq += 1;
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
