//! Cleaning up AI data: log entries, recordings, change backups and runs-database rows.
//!
//! The audit log is a signed hash chain, so entries are never deleted outright: a pruned
//! entry becomes a stub and the leading stubs fold into `audit/anchor.json` (see
//! [`super::audit`]). Everything else is plain files and rows keyed by run id.
//!
//! Rules that hold for every mode:
//!
//! * a run that is active is never touched. A run is active when it has a live control
//!   socket, or when nothing says it ended (no `RunEnded` in the log, no end in the runs
//!   database) and it started less than 24 hours ago;
//! * the log is only rewritten under the exclusive lock appends use, and the plan is
//!   rebuilt under that lock, so a stale plan is never trusted;
//! * in archive mode the archive is complete and fsynced before anything is deleted.

use super::audit::{self, LockedLog, LogLine};
use super::feed::event_run_id;
use super::report::{run_casts, safe_part};
use super::store::AiStore;
use super::types::AiEvent;
use super::{ai_dir, control_client};
use crate::config::AppConfig;
use crate::services::recording::recordings_dir;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A run with no end that started longer ago than this, and no live socket, is abandoned.
const ABANDONED_AFTER_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PruneSelection {
    /// Runs that ended before now - days.
    OlderThan { days: u32 },
    /// These run ids (full or unique prefix, like `bssh ai log <run>`).
    Runs { ids: Vec<String> },
    /// Every ended run.
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PruneMode {
    /// Remove the data (log entries become stubs/anchor).
    #[default]
    Delete,
    /// Copy everything to an archive dir first, then remove.
    Archive,
    /// Selection must be All: reset the chain and remove every AI artifact.
    Wipe,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruneWhat {
    /// Log entries (and the change records the reports are built from).
    pub audit: bool,
    /// `.cast` files.
    pub recordings: bool,
    /// Change backups (undo points are lost!).
    pub backups: bool,
    /// Runs/approvals/grants/counters rows.
    pub runs_db: bool,
}

impl Default for PruneWhat {
    fn default() -> Self {
        Self {
            audit: true,
            recordings: true,
            backups: true,
            runs_db: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneRequest {
    pub selection: PruneSelection,
    pub mode: PruneMode,
    #[serde(default)]
    pub what: PruneWhat,
    /// Archive destination; None = ai_dir/archive/<stamp>/.
    pub archive_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunedRun {
    pub run_id: String,
    pub client: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub entries: u64,
    pub recordings: u64,
    pub backup_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunePlan {
    pub runs: Vec<PrunedRun>,
    /// Active runs that matched but are skipped.
    pub skipped_active: Vec<String>,
    pub entries: u64,
    pub recordings: u64,
    /// Total bytes freed (approx).
    pub bytes: u64,
    pub log_bytes_before: u64,
    pub mode: PruneMode,
    pub archive_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneOutcome {
    pub plan: PrunePlan,
    pub log_bytes_after: u64,
    pub archive_dir: Option<PathBuf>,
    /// `verify()` after the prune.
    pub verify_ok: bool,
}

/// Size of the AI data on disk, for the UI header.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiStorage {
    pub log_bytes: u64,
    pub recordings_bytes: u64,
    pub backups_bytes: u64,
    pub runs_db_bytes: u64,
    pub archives_bytes: u64,
    pub runs: u64,
    pub oldest: Option<String>,
}

// ------------------------------------------------------------------ public API

/// Dry run: what [`execute`] would do. Reads only.
///
/// # Errors
/// Returns an error when the selection names no run (or an ambiguous one), when `Wipe` is
/// combined with a narrower selection, or when the log or the runs database cannot be read.
pub fn plan(config: &AppConfig, req: &PruneRequest) -> Result<PrunePlan> {
    let lines = audit::read_lines(config)?;
    Ok(prepare(config, req, &lines, Utc::now())?.plan)
}

/// Do it. Re-plans under the lock (never trusts a stale plan).
///
/// # Errors
/// Returns an error when the plan is invalid (see [`plan`]), when a wipe finds active runs,
/// or when the archive, the log rewrite or a deletion fails. A failure after the archive was
/// written leaves a complete archive and a partly pruned store; running again finishes it.
pub fn execute(config: &AppConfig, req: &PruneRequest) -> Result<PruneOutcome> {
    let mut log = LockedLog::lock(config)?;
    let lines = log.lines()?;
    let prep = prepare(config, req, &lines, Utc::now())?;
    let wipe = req.mode == PruneMode::Wipe;
    if wipe && !prep.plan.skipped_active.is_empty() {
        anyhow::bail!(
            "cannot wipe while {} run(s) are active: {}. Stop them, or prune only the ended runs",
            prep.plan.skipped_active.len(),
            prep.plan.skipped_active.join(", ")
        );
    }
    let mut archive_dir = None;
    if !prep.chosen.is_empty() || wipe {
        if req.mode == PruneMode::Archive {
            archive_dir = Some(write_archive(config, req, &prep, &lines)?);
        }
        if wipe {
            apply_wipe(config, &mut log)?;
        } else {
            apply_prune(config, &mut log, &prep)?;
        }
    }
    drop(log);
    let after = file_len(&audit::events_path(config)) + file_len(&audit::anchor_path(config));
    let verify_ok = audit::verify(config)?.ok;
    let mut plan = prep.plan;
    plan.archive_dir = archive_dir.clone();
    Ok(PruneOutcome {
        plan,
        log_bytes_after: after,
        archive_dir,
        verify_ok,
    })
}

/// Size of the AI data on disk.
///
/// # Errors
/// Returns an error when the log or the runs database cannot be read.
pub fn storage(config: &AppConfig) -> Result<AiStorage> {
    let ai = ai_dir(config);
    let lines = audit::read_lines(config)?;
    let runs = gather(config, &lines)?;
    let oldest = runs
        .iter()
        .filter_map(|r| Some((parse_ts(r.started.as_deref()?)?, r.started.clone()?)))
        .min_by_key(|(t, _)| *t)
        .map(|(_, s)| s);
    let db = ai.join("runs.db");
    let db_bytes: u64 = ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let mut name = db.clone().into_os_string();
            name.push(suffix);
            file_len(Path::new(&name))
        })
        .sum();
    Ok(AiStorage {
        log_bytes: file_len(&audit::events_path(config)) + file_len(&audit::anchor_path(config)),
        recordings_bytes: all_casts(config).iter().map(|p| file_len(p)).sum(),
        backups_bytes: tree_size(&ai.join("backups")),
        runs_db_bytes: db_bytes,
        archives_bytes: tree_size(&ai.join("archive")),
        runs: runs.len() as u64,
        oldest,
    })
}

// ------------------------------------------------------------------ gathering

/// Everything known about one run, from the log and the runs database.
#[derive(Debug, Clone)]
struct RunAcc {
    id: String,
    client: Option<String>,
    started: Option<String>,
    /// From `RunEnded` or the runs database.
    ended: Option<String>,
    first_ts: Option<String>,
    last_ts: Option<String>,
    /// Sequence numbers of its live (not stubbed) log entries.
    seqs: Vec<u64>,
    /// Bytes its log entries would free as stubs.
    log_savings: u64,
    in_db: bool,
}

impl RunAcc {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            client: None,
            started: None,
            ended: None,
            first_ts: None,
            last_ts: None,
            seqs: Vec::new(),
            log_savings: 0,
            in_db: false,
        }
    }
}

/// Runs seen in the log or the runs database, in order of first appearance.
fn gather(config: &AppConfig, lines: &[LogLine]) -> Result<Vec<RunAcc>> {
    let mut runs: Vec<RunAcc> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut slot = |runs: &mut Vec<RunAcc>, id: &str| -> usize {
        *index.entry(id.to_string()).or_insert_with(|| {
            runs.push(RunAcc::new(id));
            runs.len() - 1
        })
    };
    for l in lines.iter().filter(|l| !l.entry.pruned) {
        let Ok(event) = serde_json::from_value::<AiEvent>(l.entry.event.clone()) else {
            continue;
        };
        let i = slot(&mut runs, event_run_id(&event));
        let r = &mut runs[i];
        let ts = &l.entry.ts;
        r.first_ts.get_or_insert_with(|| ts.clone());
        r.last_ts = Some(ts.clone());
        r.seqs.push(l.entry.seq);
        // A stub keeps the line minus the event: `null` and `,"pruned":true`.
        r.log_savings += (l.entry.event.to_string().len() as u64).saturating_sub(18);
        match event {
            AiEvent::RunStarted { client, .. } => {
                r.client = Some(client);
                r.started = Some(ts.clone());
            }
            AiEvent::RunEnded { .. } => {
                r.ended.get_or_insert_with(|| ts.clone());
            }
            _ => {}
        }
    }
    if ai_dir(config).join("runs.db").exists() {
        let store = AiStore::open(config)?;
        for rec in store.list_runs(false, i64::MAX as usize)? {
            let i = slot(&mut runs, &rec.id);
            let r = &mut runs[i];
            r.in_db = true;
            r.client.get_or_insert(rec.client);
            r.started.get_or_insert(rec.started);
            if r.ended.is_none() {
                r.ended = rec.ended;
            }
        }
    }
    Ok(runs)
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// A run id that is safe to use as a directory name.
fn safe_name(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0'])
}

/// Whether something is listening on the run's control socket.
#[cfg(unix)]
fn is_live(ai: &Path, id: &str) -> bool {
    safe_name(id)
        && std::os::unix::net::UnixStream::connect(control_client::socket_path(ai, id)).is_ok()
}

#[cfg(not(unix))]
fn is_live(_ai: &Path, _id: &str) -> bool {
    false
}

/// When a run ended, or `None` while it is active.
fn ended_at(r: &RunAcc, ai: &Path, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if is_live(ai, &r.id) {
        return None;
    }
    if let Some(e) = &r.ended {
        return Some(parse_ts(e).unwrap_or(now));
    }
    // Abandoned: no end recorded, no socket, and old. Its last sign of life is its end.
    let began = parse_ts(r.started.as_deref().or(r.first_ts.as_deref())?)?;
    if now - began > Duration::hours(ABANDONED_AFTER_HOURS) {
        let last = r.last_ts.as_deref().and_then(parse_ts).unwrap_or(began);
        return Some(last.max(began));
    }
    None
}

// ------------------------------------------------------------------ planning

/// A plan plus what executing it needs.
struct Prepared {
    plan: PrunePlan,
    chosen: Vec<RunAcc>,
    /// What each chosen run owns on disk, parallel to `chosen`.
    footprints: Vec<Footprint>,
    /// Log entries to stub (empty unless `what.audit`).
    stub_seqs: HashSet<u64>,
    what: PruneWhat,
}

/// Files and directories of one run that a prune removes.
struct Footprint {
    casts: Vec<PathBuf>,
    cast_bytes: u64,
    backups: Option<PathBuf>,
    backup_bytes: u64,
    changes: Option<PathBuf>,
    changes_bytes: u64,
}

fn run_dir(root: PathBuf, id: &str) -> Option<PathBuf> {
    let p = root.join(id);
    (safe_name(id) && p.is_dir()).then_some(p)
}

fn footprint(config: &AppConfig, id: &str, others: &[RunAcc], what: &PruneWhat) -> Footprint {
    let ai = ai_dir(config);
    let mut casts = Vec::new();
    if what.recordings {
        // `ai-<run>-<call>.cast`: another run whose id extends this one would match too.
        let own = safe_part(id);
        let longer: Vec<String> = others
            .iter()
            .map(|o| safe_part(&o.id))
            .filter(|o| o.len() > own.len() && o.starts_with(&format!("{own}-")))
            .map(|o| format!("ai-{o}-"))
            .collect();
        casts = run_casts(config, id)
            .into_iter()
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                !longer.iter().any(|l| name.starts_with(l.as_str()))
            })
            .collect();
    }
    let backups = what
        .backups
        .then(|| run_dir(ai.join("backups"), id))
        .flatten();
    let changes = what
        .audit
        .then(|| run_dir(ai.join("changes"), id))
        .flatten();
    Footprint {
        cast_bytes: casts.iter().map(|p| file_len(p)).sum(),
        backup_bytes: backups.as_deref().map(tree_size).unwrap_or(0),
        changes_bytes: changes.as_deref().map(tree_size).unwrap_or(0),
        casts,
        backups,
        changes,
    }
}

fn all_casts(config: &AppConfig) -> Vec<PathBuf> {
    std::fs::read_dir(recordings_dir(config))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("ai-") && n.ends_with(".cast") && e.path().is_file()
        })
        .map(|e| e.path())
        .collect()
}

/// Resolve a full id or a unique prefix among known runs.
fn resolve<'a>(runs: &'a [RunAcc], wanted: &str) -> Result<&'a RunAcc> {
    if let Some(r) = runs.iter().find(|r| r.id == wanted) {
        return Ok(r);
    }
    let mut m = runs
        .iter()
        .filter(|r| !wanted.is_empty() && r.id.starts_with(wanted));
    match (m.next(), m.next()) {
        (Some(r), None) => Ok(r),
        (None, _) => anyhow::bail!("no run '{wanted}' in the audit log or the runs database"),
        _ => anyhow::bail!("run id '{wanted}' is ambiguous"),
    }
}

fn prepare(
    config: &AppConfig,
    req: &PruneRequest,
    lines: &[LogLine],
    now: DateTime<Utc>,
) -> Result<Prepared> {
    let wipe = req.mode == PruneMode::Wipe;
    if wipe && req.selection != PruneSelection::All {
        anyhow::bail!("a wipe removes everything: select all runs");
    }
    // A wipe takes everything, whatever `what` says.
    let what = if wipe {
        PruneWhat::default()
    } else {
        req.what.clone()
    };
    let ai = ai_dir(config);
    let mut runs = gather(config, lines)?;
    runs.sort_by_key(|r| {
        r.started
            .as_deref()
            .and_then(parse_ts)
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    });

    let ended: Vec<Option<DateTime<Utc>>> = runs.iter().map(|r| ended_at(r, &ai, now)).collect();
    let mut skipped_active = Vec::new();
    let mut picked: Vec<usize> = Vec::new();
    match &req.selection {
        PruneSelection::All => {
            for (i, e) in ended.iter().enumerate() {
                if e.is_some() {
                    picked.push(i);
                } else {
                    skipped_active.push(runs[i].id.clone());
                }
            }
        }
        PruneSelection::OlderThan { days } => {
            let cutoff = now - Duration::days(i64::from(*days));
            for (i, e) in ended.iter().enumerate() {
                match e {
                    Some(t) if *t < cutoff => picked.push(i),
                    None if runs[i]
                        .started
                        .as_deref()
                        .and_then(parse_ts)
                        .is_some_and(|s| s < cutoff) =>
                    {
                        skipped_active.push(runs[i].id.clone());
                    }
                    _ => {}
                }
            }
        }
        PruneSelection::Runs { ids } => {
            let mut seen = HashSet::new();
            for wanted in ids {
                let id = resolve(&runs, wanted)?.id.clone();
                let i = runs
                    .iter()
                    .position(|r| r.id == id)
                    .context("run vanished")?;
                if !seen.insert(i) {
                    continue;
                }
                if ended[i].is_some() {
                    picked.push(i);
                } else {
                    skipped_active.push(id);
                }
            }
            picked.sort_unstable();
        }
    }

    let chosen: Vec<RunAcc> = picked.iter().map(|&i| runs[i].clone()).collect();
    let mut pruned = Vec::with_capacity(chosen.len());
    let (mut entries, mut recordings, mut bytes) = (0u64, 0u64, 0u64);
    let mut stub_seqs = HashSet::new();
    let mut footprints = Vec::with_capacity(chosen.len());
    for r in &chosen {
        let fp = footprint(config, &r.id, &runs, &what);
        let n = if what.audit { r.seqs.len() as u64 } else { 0 };
        if what.audit {
            stub_seqs.extend(r.seqs.iter().copied());
            bytes += r.log_savings + fp.changes_bytes;
        }
        entries += n;
        recordings += fp.casts.len() as u64;
        bytes += fp.cast_bytes + fp.backup_bytes;
        pruned.push(PrunedRun {
            run_id: r.id.clone(),
            client: r.client.clone(),
            started: r.started.clone(),
            ended: r.ended.clone(),
            entries: n,
            recordings: fp.casts.len() as u64,
            backup_bytes: fp.backup_bytes,
        });
        footprints.push(fp);
    }
    let events = audit::events_path(config);
    if wipe {
        // Everything on disk, not just what the known runs account for.
        entries = lines.iter().filter(|l| !l.entry.pruned).count() as u64;
        let casts = all_casts(config);
        recordings = casts.len() as u64;
        bytes = file_len(&events)
            + file_len(&audit::anchor_path(config))
            + casts.iter().map(|p| file_len(p)).sum::<u64>()
            + tree_size(&ai.join("backups"))
            + tree_size(&ai.join("changes"))
            + file_len(&ai.join("runs.db"));
    }
    let archive_dir = (req.mode == PruneMode::Archive).then(|| {
        req.archive_dir
            .clone()
            .unwrap_or_else(|| default_archive_dir(&ai))
    });
    Ok(Prepared {
        plan: PrunePlan {
            runs: pruned,
            skipped_active,
            entries,
            recordings,
            bytes,
            log_bytes_before: file_len(&events),
            mode: req.mode,
            archive_dir,
        },
        chosen,
        footprints,
        stub_seqs,
        what,
    })
}

fn default_archive_dir(ai: &Path) -> PathBuf {
    ai.join("archive")
        .join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string())
}

// ------------------------------------------------------------------ applying

fn remove_file_if_exists(p: &Path) -> Result<()> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("remove {}", p.display())),
    }
}

fn remove_dir_if_exists(p: &Path) -> Result<()> {
    match std::fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("remove {}", p.display())),
    }
}

/// Stub the log, then remove recordings, backups, change records and database rows of the
/// chosen runs. The log goes first: it is the part that must stay consistent.
fn apply_prune(config: &AppConfig, log: &mut LockedLog, prep: &Prepared) -> Result<()> {
    if prep.what.audit && !prep.stub_seqs.is_empty() {
        log.rewrite(&prep.stub_seqs)?;
    }
    for fp in &prep.footprints {
        for c in &fp.casts {
            remove_file_if_exists(c)?;
        }
        for d in fp.backups.iter().chain(fp.changes.iter()) {
            remove_dir_if_exists(d)?;
        }
    }
    if prep.what.runs_db && ai_dir(config).join("runs.db").exists() {
        let ids: Vec<String> = prep.chosen.iter().map(|r| r.id.clone()).collect();
        AiStore::open(config)?.delete_runs(&ids)?;
    }
    Ok(())
}

/// Remove every AI artifact; the audit key stays.
fn apply_wipe(config: &AppConfig, log: &mut LockedLog) -> Result<()> {
    let ai = ai_dir(config);
    log.wipe()?;
    for c in all_casts(config) {
        remove_file_if_exists(&c)?;
    }
    remove_dir_if_exists(&ai.join("backups"))?;
    remove_dir_if_exists(&ai.join("changes"))?;
    if ai.join("runs.db").exists() {
        AiStore::open(config)?.delete_everything()?;
    }
    Ok(())
}

// ------------------------------------------------------------------ archive

#[derive(Serialize)]
struct ManifestRun<'a> {
    run_id: &'a str,
    client: Option<&'a str>,
    started: Option<&'a str>,
    ended: Option<&'a str>,
    entries: u64,
    seq_first: Option<u64>,
    seq_last: Option<u64>,
    recordings: u64,
    backup_bytes: u64,
}

#[derive(Serialize)]
struct Manifest<'a> {
    created_at: String,
    bssh_version: &'a str,
    mode: PruneMode,
    what: &'a PruneWhat,
    entries: u64,
    recordings: u64,
    runs: Vec<ManifestRun<'a>>,
}

fn create_private_dir(p: &Path) -> Result<()> {
    std::fs::create_dir_all(p).with_context(|| format!("create {}", p.display()))?;
    crate::config::enforce_secure_dir(p);
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(bytes)?;
    f.sync_all()
        .with_context(|| format!("sync {}", path.display()))
}

fn sync_dir(p: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(p) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = p;
}

/// Copy a tree without following symlinks, fsyncing every file and directory.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    create_private_dir(dst)?;
    for e in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let e = e?;
        let to = dst.join(e.file_name());
        let ty = e.file_type()?;
        if ty.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else if ty.is_file() {
            copy_file(&e.path(), &to)?;
        }
    }
    sync_dir(dst);
    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    let bytes = std::fs::read(src).with_context(|| format!("read {}", src.display()))?;
    write_synced(dst, &bytes)
}

/// Write the archive of everything `prep` will remove, and return its directory. The manifest
/// goes last, so a directory with a manifest is a complete archive.
fn write_archive(
    config: &AppConfig,
    req: &PruneRequest,
    prep: &Prepared,
    lines: &[LogLine],
) -> Result<PathBuf> {
    let ai = ai_dir(config);
    let dir = std::path::absolute(
        req.archive_dir
            .clone()
            .unwrap_or_else(|| default_archive_dir(&ai)),
    )?;
    // Never inside what gets deleted.
    for doomed in [
        recordings_dir(config),
        ai.join("backups"),
        ai.join("changes"),
        audit::audit_dir(config),
    ] {
        let doomed = std::path::absolute(doomed)?;
        anyhow::ensure!(
            !dir.starts_with(&doomed),
            "the archive cannot live inside {}",
            doomed.display()
        );
    }
    anyhow::ensure!(
        !dir.join("manifest.json").exists() && !dir.join("events.jsonl").exists(),
        "{} already holds an archive",
        dir.display()
    );
    create_private_dir(&dir)?;

    // Original lines, byte for byte, so their hashes match the stubs and the anchor.
    if prep.what.audit {
        let mut text = String::new();
        for l in lines
            .iter()
            .filter(|l| prep.stub_seqs.contains(&l.entry.seq))
        {
            text.push_str(&l.text);
            text.push('\n');
        }
        write_synced(&dir.join("events.jsonl"), text.as_bytes())?;
    }
    for (r, fp) in prep.chosen.iter().zip(&prep.footprints) {
        if !fp.casts.is_empty() {
            create_private_dir(&dir.join("recordings"))?;
        }
        for c in &fp.casts {
            if let Some(name) = c.file_name() {
                copy_file(c, &dir.join("recordings").join(name))?;
            }
        }
        if let Some(b) = &fp.backups {
            copy_tree(b, &dir.join("backups").join(&r.id))?;
        }
        if let Some(c) = &fp.changes {
            copy_tree(c, &dir.join("changes").join(&r.id))?;
        }
    }
    for sub in ["recordings", "backups", "changes"] {
        sync_dir(&dir.join(sub));
    }

    let seq_range = |r: &RunAcc| (r.seqs.iter().min().copied(), r.seqs.iter().max().copied());
    let manifest = Manifest {
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        bssh_version: env!("CARGO_PKG_VERSION"),
        mode: req.mode,
        what: &prep.what,
        entries: prep.plan.entries,
        recordings: prep.plan.recordings,
        runs: prep
            .chosen
            .iter()
            .zip(&prep.plan.runs)
            .map(|(r, p)| {
                let (first, last) = if prep.what.audit {
                    seq_range(r)
                } else {
                    (None, None)
                };
                ManifestRun {
                    run_id: &r.id,
                    client: r.client.as_deref(),
                    started: r.started.as_deref(),
                    ended: r.ended.as_deref(),
                    entries: p.entries,
                    seq_first: first,
                    seq_last: last,
                    recordings: p.recordings,
                    backup_bytes: p.backup_bytes,
                }
            })
            .collect(),
    };
    write_synced(
        &dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )?;
    sync_dir(&dir);
    if let Some(parent) = dir.parent() {
        sync_dir(parent);
    }
    Ok(dir)
}

// ------------------------------------------------------------------ sizes

fn file_len(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Total size of the files under `p`, symlinks not followed. Missing = 0.
fn tree_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if md.is_file() {
        return md.len();
    }
    if !md.is_dir() {
        return 0;
    }
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| tree_size(&e.path()))
        .sum()
}

#[cfg(test)]
mod tests;
