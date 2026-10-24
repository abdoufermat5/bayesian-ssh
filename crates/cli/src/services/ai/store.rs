//! Runs, approvals and grants in SQLite (`<ai_dir>/runs.db`).
//!
//! Several processes share this file (one `bssh ai mcp` per agent, plus the CLI and the
//! desktop app), so the database runs in WAL mode with a busy timeout. The file is 0600 and
//! its directory 0700.
//!
//! Owner: W3. Used by the policy engine (W3), the MCP server (W4: runs, changes), the audit
//! and reports (W6) and the desktop app (W7: grants, history).

use super::types::{ApprovalOutcome, ApprovalRequest, Grant, ToolName};
use crate::config::{enforce_secure_dir, enforce_secure_file, AppConfig};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// Kinds of per-run counters (calls per minute, denials per minute...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterKind {
    /// Every tool call that reached the policy engine.
    Call,
    /// Every call the engine denied.
    Denial,
    /// A call aimed at a host outside the policy scope (probing).
    Probe,
}

impl CounterKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Denial => "denial",
            Self::Probe => "probe",
        }
    }
}

/// One row of `runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub client: String,
    pub on_behalf_of: String,
    pub policy: String,
    /// RFC 3339.
    pub started: String,
    pub ended: Option<String>,
    pub reason: Option<String>,
}

/// Counters older than this are dropped on write.
const COUNTER_RETENTION_SECS: i64 = 3600;

/// SQLite store for runs, approval requests, grants and budget counters.
pub struct AiStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl AiStore {
    /// Open (and create) `<ai_dir>/runs.db` for this environment.
    pub fn open(config: &AppConfig) -> Result<Self> {
        Self::open_in(&super::ai_dir(config))
    }

    /// Open (and create) `<dir>/runs.db`. Used by tests with a temp dir.
    pub fn open_in(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        enforce_secure_dir(dir);
        let path = dir.join("runs.db");
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(15))?;
        // Switching to WAL and creating tables need a lock that the busy timeout does not
        // always cover when several processes open a fresh database at once: retry.
        let mut attempt = 0;
        loop {
            // `journal_mode` returns a row, so it can't go through execute_batch.
            let init = conn
                .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
                .map(|_| ())
                // NORMAL is safe in WAL mode and avoids an fsync per write.
                .and_then(|_| conn.execute_batch("PRAGMA synchronous=NORMAL"))
                .and_then(|_| conn.execute_batch(SCHEMA));
            match init {
                Ok(()) => break,
                Err(e) if attempt < 100 && is_busy(&e) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e.into()),
            }
        }
        enforce_secure_file(&path);
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Where the database lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("AI store lock poisoned"))
    }

    // ------------------------------------------------------------ runs

    /// Record a new run. Starting the same id twice is an error.
    pub fn start_run(&self, run: &RunRecord) -> Result<()> {
        self.db()?.execute(
            "INSERT INTO runs (id, client, on_behalf_of, policy, started, ended, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.id,
                run.client,
                run.on_behalf_of,
                run.policy,
                run.started,
                run.ended,
                run.reason
            ],
        )?;
        Ok(())
    }

    /// Mark a run as ended. A run that already ended keeps its first reason.
    pub fn end_run(&self, run_id: &str, reason: &str) -> Result<()> {
        self.db()?.execute(
            "UPDATE runs SET ended = ?2, reason = ?3 WHERE id = ?1 AND ended IS NULL",
            params![run_id, Utc::now().to_rfc3339(), reason],
        )?;
        Ok(())
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let db = self.db()?;
        let row = db
            .query_row(
                "SELECT id, client, on_behalf_of, policy, started, ended, reason
                 FROM runs WHERE id = ?1",
                params![run_id],
                row_to_run,
            )
            .optional()?;
        Ok(row)
    }

    /// Most recent runs first. `active_only` keeps runs that have not ended.
    pub fn list_runs(&self, active_only: bool, limit: usize) -> Result<Vec<RunRecord>> {
        let db = self.db()?;
        let sql = if active_only {
            "SELECT id, client, on_behalf_of, policy, started, ended, reason
             FROM runs WHERE ended IS NULL ORDER BY started DESC LIMIT ?1"
        } else {
            "SELECT id, client, on_behalf_of, policy, started, ended, reason
             FROM runs ORDER BY started DESC LIMIT ?1"
        };
        let mut stmt = db.prepare(sql)?;
        let rows = stmt
            .query_map(params![limit as i64], row_to_run)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------ approvals

    /// Persist a request as `pending`.
    pub fn insert_request(&self, req: &ApprovalRequest) -> Result<()> {
        self.db()?.execute(
            "INSERT OR REPLACE INTO approvals
             (request_id, run_id, client, tier, subject, created_at, expires_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
            params![
                req.request_id,
                req.run_id,
                req.client,
                serde_json::to_string(&req.tier)?,
                serde_json::to_string(&req.subject)?,
                req.created_at,
                req.expires_at
            ],
        )?;
        Ok(())
    }

    /// Store the final outcome of a request (`approved`, `denied` or `expired`).
    pub fn record_outcome(&self, request_id: &str, outcome: &ApprovalOutcome) -> Result<()> {
        let (status, by) = match outcome {
            ApprovalOutcome::Approved { by, .. } => ("approved", Some(by.as_str())),
            ApprovalOutcome::Denied { by, .. } => ("denied", Some(by.as_str())),
            ApprovalOutcome::Expired => ("expired", None),
        };
        self.db()?.execute(
            "UPDATE approvals SET status = ?2, outcome = ?3, decided_at = ?4, decided_by = ?5
             WHERE request_id = ?1",
            params![
                request_id,
                status,
                serde_json::to_string(outcome)?,
                Utc::now().to_rfc3339(),
                by
            ],
        )?;
        Ok(())
    }

    /// `pending`, `approved`, `denied` or `expired`; `None` for an unknown id.
    pub fn request_status(&self, request_id: &str) -> Result<Option<String>> {
        let db = self.db()?;
        Ok(db
            .query_row(
                "SELECT status FROM approvals WHERE request_id = ?1",
                params![request_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The recorded outcome of a decided request.
    pub fn get_outcome(&self, request_id: &str) -> Result<Option<ApprovalOutcome>> {
        let db = self.db()?;
        let raw: Option<Option<String>> = db
            .query_row(
                "SELECT outcome FROM approvals WHERE request_id = ?1",
                params![request_id],
                |r| r.get(0),
            )
            .optional()?;
        match raw.flatten() {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    /// Requests still waiting for a decision, oldest first. `run_id = None` = all runs.
    pub fn pending_requests(&self, run_id: Option<&str>) -> Result<Vec<ApprovalRequest>> {
        let db = self.db()?;
        let mut stmt = db.prepare(
            "SELECT request_id, run_id, client, tier, subject, created_at, expires_at
             FROM approvals
             WHERE status = 'pending' AND (?1 IS NULL OR run_id = ?1)
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![run_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (request_id, run_id, client, tier, subject, created_at, expires_at) in rows {
            out.push(ApprovalRequest {
                request_id,
                run_id,
                client,
                tier: serde_json::from_str(&tier)?,
                subject: serde_json::from_str(&subject)?,
                created_at,
                expires_at,
            });
        }
        Ok(out)
    }

    /// Mark every request of a run that is still pending as expired (run ended or crashed).
    pub fn expire_pending(&self, run_id: &str) -> Result<usize> {
        let n = self.db()?.execute(
            "UPDATE approvals SET status = 'expired', outcome = ?2, decided_at = ?3
             WHERE run_id = ?1 AND status = 'pending'",
            params![
                run_id,
                serde_json::to_string(&ApprovalOutcome::Expired)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(n)
    }

    // ------------------------------------------------------------ grants

    /// Store a grant. An empty `grant_id` is not allowed.
    pub fn add_grant(&self, grant: &Grant) -> Result<()> {
        anyhow::ensure!(!grant.grant_id.is_empty(), "grant without id");
        self.db()?.execute(
            "INSERT OR REPLACE INTO grants
             (grant_id, run_id, tool, hosts, target, expires_at, created_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![
                grant.grant_id,
                grant.run_id,
                serde_json::to_string(&grant.tool)?,
                grant.hosts,
                grant.target,
                grant.expires_at,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Grants that are not expired or revoked and apply to `run_id`
    /// (run-scoped grants of that run, plus grants with no run).
    /// `run_id = None` lists every active grant.
    pub fn active_grants(&self, run_id: Option<&str>) -> Result<Vec<Grant>> {
        let db = self.db()?;
        let mut stmt = db.prepare(
            "SELECT grant_id, run_id, tool, hosts, target, expires_at FROM grants
             WHERE revoked = 0 AND (?1 IS NULL OR run_id IS NULL OR run_id = ?1)
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![run_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let now = Utc::now();
        let mut out = Vec::new();
        for (grant_id, run_id, tool, hosts, target, expires_at) in rows {
            let alive = DateTime::parse_from_rfc3339(&expires_at)
                .map(|t| t.with_timezone(&Utc) > now)
                .unwrap_or(false);
            if !alive {
                continue;
            }
            let tool: ToolName = serde_json::from_str(&tool)?;
            out.push(Grant {
                grant_id,
                run_id,
                tool,
                hosts,
                target,
                expires_at,
            });
        }
        Ok(out)
    }

    /// Revoke a grant. Returns false when the id is unknown or already revoked.
    pub fn revoke_grant(&self, grant_id: &str) -> Result<bool> {
        let n = self.db()?.execute(
            "UPDATE grants SET revoked = 1 WHERE grant_id = ?1 AND revoked = 0",
            params![grant_id],
        )?;
        Ok(n > 0)
    }

    // ------------------------------------------------------------ counters

    /// Count one event of `kind` for the run, now.
    pub fn note(&self, run_id: &str, kind: CounterKind) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        let db = self.db()?;
        db.execute(
            "INSERT INTO counters (run_id, kind, at) VALUES (?1, ?2, ?3)",
            params![run_id, kind.as_str(), now],
        )?;
        // Prune now and then, not on every call.
        if db.last_insert_rowid() % 64 == 0 {
            db.execute(
                "DELETE FROM counters WHERE at < ?1",
                params![now - COUNTER_RETENTION_SECS * 1000],
            )?;
        }
        Ok(())
    }

    /// Events of `kind` for the run in the last `window` (at most an hour is kept).
    pub fn count_within(&self, run_id: &str, kind: CounterKind, window: Duration) -> Result<u32> {
        let since = Utc::now().timestamp_millis() - window.as_millis() as i64;
        let n: i64 = self.db()?.query_row(
            "SELECT COUNT(*) FROM counters WHERE run_id = ?1 AND kind = ?2 AND at >= ?3",
            params![run_id, kind.as_str(), since],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// Record an applied change (W4 calls this after `apply_change` / `run` / `edit_file`
    /// actually changed something). Feeds the `changes_per_run` budget.
    pub fn note_change(&self, run_id: &str, label: &str) -> Result<()> {
        self.db()?.execute(
            "INSERT INTO changes (run_id, label, at) VALUES (?1, ?2, ?3)",
            params![run_id, label, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Changes applied so far in a run (never ages out, unlike the rate counters).
    pub fn changes_in_run(&self, run_id: &str) -> Result<u32> {
        let n: i64 = self.db()?.query_row(
            "SELECT COUNT(*) FROM changes WHERE run_id = ?1",
            params![run_id],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }
}

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(f.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        id: r.get(0)?,
        client: r.get(1)?,
        on_behalf_of: r.get(2)?,
        policy: r.get(3)?,
        started: r.get(4)?,
        ended: r.get(5)?,
        reason: r.get(6)?,
    })
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    client TEXT NOT NULL,
    on_behalf_of TEXT NOT NULL,
    policy TEXT NOT NULL,
    started TEXT NOT NULL,
    ended TEXT,
    reason TEXT
);
CREATE TABLE IF NOT EXISTS approvals (
    request_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    client TEXT NOT NULL,
    tier TEXT NOT NULL,
    subject TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    status TEXT NOT NULL,
    outcome TEXT,
    decided_at TEXT,
    decided_by TEXT
);
CREATE INDEX IF NOT EXISTS idx_approvals_run ON approvals (run_id, status);
CREATE TABLE IF NOT EXISTS grants (
    grant_id TEXT PRIMARY KEY,
    run_id TEXT,
    tool TEXT NOT NULL,
    hosts TEXT NOT NULL,
    target TEXT,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS counters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_counters ON counters (run_id, kind, at);
CREATE TABLE IF NOT EXISTS changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    label TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_changes ON changes (run_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{ApprovalSubject, Scope, Tier};

    fn run(id: &str) -> RunRecord {
        RunRecord {
            id: id.into(),
            client: "claude-code 1.0".into(),
            on_behalf_of: "alice".into(),
            policy: "default".into(),
            started: Utc::now().to_rfc3339(),
            ended: None,
            reason: None,
        }
    }

    fn request(id: &str, run_id: &str) -> ApprovalRequest {
        ApprovalRequest {
            request_id: id.into(),
            run_id: run_id.into(),
            client: "c".into(),
            tier: Tier::OutOfBand,
            subject: ApprovalSubject::Access {
                scope: Scope::default(),
                reason: "why".into(),
                duration: "30m".into(),
            },
            created_at: Utc::now().to_rfc3339(),
            expires_at: (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
        }
    }

    fn grant(id: &str, run_id: Option<&str>, mins: i64) -> Grant {
        Grant {
            grant_id: id.into(),
            run_id: run_id.map(String::from),
            tool: ToolName::Service,
            hosts: "web-*".into(),
            target: Some("nginx".into()),
            expires_at: (Utc::now() + chrono::Duration::minutes(mins)).to_rfc3339(),
        }
    }

    #[test]
    fn open_creates_db_with_private_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ai");
        let store = AiStore::open_in(&dir).unwrap();
        assert!(store.path().exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let f = std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            let d = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(f, 0o600);
            assert_eq!(d, 0o700);
        }
    }

    #[test]
    fn two_handles_share_data() {
        let tmp = tempfile::tempdir().unwrap();
        let a = AiStore::open_in(tmp.path()).unwrap();
        let b = AiStore::open_in(tmp.path()).unwrap();
        a.start_run(&run("r1")).unwrap();
        assert!(b.get_run("r1").unwrap().is_some());
    }

    #[test]
    fn run_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        s.start_run(&run("r1")).unwrap();
        assert!(s.start_run(&run("r1")).is_err());
        assert_eq!(s.list_runs(true, 10).unwrap().len(), 1);
        s.end_run("r1", "done").unwrap();
        s.end_run("r1", "again").unwrap();
        let r = s.get_run("r1").unwrap().unwrap();
        assert_eq!(r.reason.as_deref(), Some("done"));
        assert!(r.ended.is_some());
        assert!(s.list_runs(true, 10).unwrap().is_empty());
        assert_eq!(s.list_runs(false, 10).unwrap().len(), 1);
        assert!(s.get_run("nope").unwrap().is_none());
    }

    #[test]
    fn approval_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        s.insert_request(&request("q1", "r1")).unwrap();
        s.insert_request(&request("q2", "r2")).unwrap();
        assert_eq!(s.pending_requests(None).unwrap().len(), 2);
        assert_eq!(s.pending_requests(Some("r1")).unwrap().len(), 1);
        assert_eq!(s.request_status("q1").unwrap().as_deref(), Some("pending"));
        let out = ApprovalOutcome::Denied {
            by: "alice".into(),
            reason: Some("no".into()),
        };
        s.record_outcome("q1", &out).unwrap();
        assert_eq!(s.request_status("q1").unwrap().as_deref(), Some("denied"));
        assert_eq!(s.get_outcome("q1").unwrap(), Some(out));
        assert_eq!(s.pending_requests(None).unwrap().len(), 1);
        assert_eq!(s.expire_pending("r2").unwrap(), 1);
        assert_eq!(s.get_outcome("q2").unwrap(), Some(ApprovalOutcome::Expired));
        assert!(s.request_status("zz").unwrap().is_none());
    }

    #[test]
    fn pending_request_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        let r = request("q1", "r1");
        s.insert_request(&r).unwrap();
        assert_eq!(s.pending_requests(None).unwrap(), vec![r]);
    }

    #[test]
    fn grants_expire_and_revoke() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        s.add_grant(&grant("g1", Some("r1"), 10)).unwrap();
        s.add_grant(&grant("g2", None, 10)).unwrap();
        s.add_grant(&grant("g3", Some("r2"), 10)).unwrap();
        s.add_grant(&grant("gold", Some("r1"), -10)).unwrap();
        let ids: Vec<_> = s
            .active_grants(Some("r1"))
            .unwrap()
            .into_iter()
            .map(|g| g.grant_id)
            .collect();
        assert_eq!(ids, vec!["g1", "g2"]);
        assert_eq!(s.active_grants(None).unwrap().len(), 3);
        assert!(s.revoke_grant("g1").unwrap());
        assert!(!s.revoke_grant("g1").unwrap());
        assert!(!s.revoke_grant("missing").unwrap());
        assert_eq!(s.active_grants(Some("r1")).unwrap().len(), 1);
        assert!(s
            .add_grant(&Grant {
                grant_id: String::new(),
                ..grant("x", None, 1)
            })
            .is_err());
    }

    #[test]
    fn counters_window_and_run_isolation() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        for _ in 0..3 {
            s.note("r1", CounterKind::Call).unwrap();
        }
        s.note("r2", CounterKind::Call).unwrap();
        s.note("r1", CounterKind::Denial).unwrap();
        let min = Duration::from_secs(60);
        assert_eq!(s.count_within("r1", CounterKind::Call, min).unwrap(), 3);
        assert_eq!(s.count_within("r2", CounterKind::Call, min).unwrap(), 1);
        assert_eq!(s.count_within("r1", CounterKind::Denial, min).unwrap(), 1);
        // Old events fall out of a short window.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            s.count_within("r1", CounterKind::Call, Duration::from_millis(5))
                .unwrap(),
            0
        );
    }

    #[test]
    fn changes_are_counted_per_run() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AiStore::open_in(tmp.path()).unwrap();
        s.note_change("r1", "restart nginx").unwrap();
        s.note_change("r1", "edit /etc/x").unwrap();
        s.note_change("r2", "x").unwrap();
        assert_eq!(s.changes_in_run("r1").unwrap(), 2);
        assert_eq!(s.changes_in_run("r3").unwrap(), 0);
    }

    #[test]
    fn concurrent_writers_do_not_lose_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let s = AiStore::open_in(&dir).unwrap();
                    for _ in 0..25 {
                        s.note(&format!("r{i}"), CounterKind::Call).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let s = AiStore::open_in(&dir).unwrap();
        for i in 0..4 {
            assert_eq!(
                s.count_within(
                    &format!("r{i}"),
                    CounterKind::Call,
                    Duration::from_secs(3600)
                )
                .unwrap(),
                25
            );
        }
    }
}
