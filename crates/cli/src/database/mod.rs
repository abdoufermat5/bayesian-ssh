use crate::config::AppConfig;
use crate::models::Connection;
use anyhow::Result;
use rusqlite::Connection as SqliteConnection;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    pub(crate) conn: SqliteConnection,
    pub(crate) window_days: u32,
}

mod alias;
mod connection;
mod search;
mod session;
mod tag;

pub use search::{Prediction, RankContext, ScoreBreakdown, SearchField};
pub use session::live_pid;

const SCHEMA_VERSION: i32 = 7;

/// How long a statement waits on a lock held by another process (the CLI
/// and the desktop app share the same database file) before failing with
/// `SQLITE_BUSY`.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Database paths whose directory, permissions, schema migrations and indexes
/// have already been set up by this process. Opening one of these again only
/// needs a fresh connection — the desktop app opens the database on every
/// command, and re-running the migration transaction (which takes the write
/// lock) each time is pure overhead.
static INITIALIZED_PATHS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

impl Database {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let path = &config.database_path;

        // Fast path: already initialized by this process. The file must still
        // exist (opening would recreate it empty) and carry the current schema
        // version (it may have been replaced, e.g. by `restore`, with an older
        // database that still needs migrating).
        let already_initialized = INITIALIZED_PATHS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(path);
        if already_initialized && path.exists() {
            let db = Self::open(config)?;
            if db.current_schema_version()? == SCHEMA_VERSION {
                return Ok(db);
            }
        }

        // Ensure database directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            crate::config::enforce_secure_dir(parent);
        }

        let db = Self::open(config)?;
        crate::config::enforce_secure_file(path);
        db.init()?;

        INITIALIZED_PATHS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.clone());

        tracing::debug!("Database initialized at {:?}", path);
        Ok(db)
    }

    fn open(config: &AppConfig) -> Result<Self> {
        let conn = SqliteConnection::open(&config.database_path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(Database {
            conn,
            window_days: config.prediction.window_days,
        })
    }

    fn init(&self) -> Result<()> {
        // Use a single transaction for the full migration so the schema is
        // always either fully applied or fully rolled back. IMMEDIATE takes
        // the write lock up front so two processes opening the database
        // concurrently wait on `busy_timeout` instead of failing with
        // SQLITE_BUSY when upgrading a read lock mid-migration.
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result: Result<()> = (|| {
            self.apply_migrations()?;
            self.create_indexes()?;
            Ok(())
        })();

        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        Ok(())
    }

    fn current_schema_version(&self) -> Result<i32> {
        // user_version is the standard SQLite PRAGMA for schema versioning.
        let v: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        Ok(v)
    }

    fn set_schema_version(&self, version: i32) -> Result<()> {
        // PRAGMA does not support bound parameters, so format the int
        // directly. `version` is always an i32 from a constant, never user input.
        let sql = format!("PRAGMA user_version = {version}");
        self.conn.execute_batch(&sql)?;
        Ok(())
    }

    fn apply_migrations(&self) -> Result<()> {
        let current = self.current_schema_version()?;
        if current > SCHEMA_VERSION {
            anyhow::bail!(
                "database schema version {current} is newer than this build supports \
                 ({SCHEMA_VERSION}); upgrade bayesian-ssh"
            );
        }

        if current < 1 {
            self.migrate_v1_create_core_tables()?;
            self.set_schema_version(1)?;
        }
        if current < 2 {
            self.migrate_v2_add_sessions_transport()?;
            self.set_schema_version(2)?;
        }
        if current < 3 {
            self.migrate_v3_create_tags_and_status()?;
            self.set_schema_version(3)?;
        }
        if current < 4 {
            self.migrate_v4_purge_orphans()?;
            self.set_schema_version(4)?;
        }
        if current < 5 {
            self.migrate_v5_session_recordings()?;
            self.set_schema_version(5)?;
        }
        if current < 6 {
            self.migrate_v6_session_context()?;
            self.set_schema_version(6)?;
        }
        if current < 7 {
            self.migrate_v7_inventory_sources()?;
            self.set_schema_version(7)?;
        }
        Ok(())
    }

    fn migrate_v1_create_core_tables(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS connections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                host TEXT NOT NULL,
                user TEXT NOT NULL,
                port INTEGER NOT NULL,
                bastion TEXT,
                bastion_user TEXT,
                use_kerberos BOOLEAN NOT NULL,
                key_path TEXT,
                created_at TEXT NOT NULL,
                last_used TEXT,
                tags TEXT NOT NULL DEFAULT '[]'
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                status TEXT NOT NULL,
                pid INTEGER,
                exit_code INTEGER,
                transport TEXT,
                FOREIGN KEY (connection_id) REFERENCES connections (id)
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS aliases (
                alias TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (connection_id) REFERENCES connections (id) ON DELETE CASCADE
            )",
            [],
        )?;
        Ok(())
    }

    fn migrate_v2_add_sessions_transport(&self) -> Result<()> {
        // The PRAGMA check ensures we only ALTER if the column is missing.
        let has_col: bool = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(sessions)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            let names: Vec<String> = rows.filter_map(Result::ok).collect();
            names.iter().any(|n| n == "transport")
        };
        if !has_col {
            self.conn
                .execute("ALTER TABLE sessions ADD COLUMN transport TEXT", [])?;
        }
        Ok(())
    }

    fn migrate_v3_create_tags_and_status(&self) -> Result<()> {
        // New normalized tags table.
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS connection_tags (
                connection_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (connection_id, tag),
                FOREIGN KEY (connection_id) REFERENCES connections (id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Add a structured status discriminator so we can query sessions
        // by outcome without parsing JSON. We keep the JSON column for
        // backward compatibility with existing readers.
        let needs_alter = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(sessions)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            let names: Vec<String> = rows.filter_map(Result::ok).collect();
            !names.iter().any(|n| n == "status_kind")
        };
        if needs_alter {
            self.conn
                .execute("ALTER TABLE sessions ADD COLUMN status_kind TEXT", [])?;
        }
        // Backfill: derive status_kind from the JSON for existing rows.
        self.conn.execute(
            "UPDATE sessions
             SET status_kind = CASE
                 WHEN status LIKE '%\"Active\"%' THEN 'active'
                 WHEN status LIKE '%\"Disconnected\"%' THEN 'disconnected'
                 WHEN status LIKE '%\"Terminated\"%' THEN 'terminated'
                 WHEN status LIKE '%\"Starting\"%' THEN 'starting'
                 WHEN status LIKE '%\"Error%' THEN 'error'
                 ELSE 'unknown'
             END
             WHERE status_kind IS NULL",
            [],
        )?;
        // One-time copy of the legacy JSON `tags` column into the new
        // table. This must NOT run on every open: once `connection_tags`
        // is authoritative, a connection whose tags were all removed would
        // otherwise get its stale JSON tags resurrected.
        self.backfill_tags_table()?;
        Ok(())
    }

    fn backfill_tags_table(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, tags FROM connections
             WHERE tags IS NOT NULL AND tags != '[]' AND id NOT IN
                 (SELECT DISTINCT connection_id FROM connection_tags)",
        )?;
        let mut rows = stmt.query([])?;
        let mut to_migrate: Vec<(String, Vec<String>)> = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let tags_json: String = row.get(1)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            if !tags.is_empty() {
                to_migrate.push((id, tags));
            }
        }
        for (id, tags) in to_migrate {
            for tag in tags.iter().filter_map(|t| Connection::normalize_tag(t)) {
                self.conn.execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
                    rusqlite::params![id, tag],
                )?;
            }
        }
        Ok(())
    }

    /// Foreign keys are not enforced (SQLite's default), so the
    /// `ON DELETE CASCADE` clauses never fired: removing a connection left
    /// its aliases and tags behind (skewing tag stats and keeping dead
    /// aliases reserved). `remove_connection` now deletes them explicitly;
    /// this cleans up rows orphaned by earlier versions.
    fn migrate_v4_purge_orphans(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM connection_tags
                 WHERE connection_id NOT IN (SELECT id FROM connections);
             DELETE FROM aliases
                 WHERE connection_id NOT IN (SELECT id FROM connections);",
        )?;
        Ok(())
    }

    /// v5: recorded sessions. `session_text` is an FTS5 index over the
    /// ANSI-stripped output of each recording, for `history --grep`.
    fn migrate_v5_session_recordings(&self) -> Result<()> {
        self.conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN recording_path TEXT;
             CREATE VIRTUAL TABLE IF NOT EXISTS session_text
                 USING fts5(session_id UNINDEXED, content, tokenize = 'unicode61');",
        )?;
        Ok(())
    }

    /// v6: launch context of each session, for predictive ranking.
    fn migrate_v6_session_context(&self) -> Result<()> {
        self.conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN cwd TEXT;
             ALTER TABLE sessions ADD COLUMN git_root TEXT;
             ALTER TABLE sessions ADD COLUMN prev_connection_id TEXT;
             ALTER TABLE sessions ADD COLUMN local_hour INTEGER;
             ALTER TABLE sessions ADD COLUMN local_weekday INTEGER;",
        )?;
        Ok(())
    }

    /// v7: inventory sources. Connections pulled from a source carry its id
    /// and the host's stable id inside it, so a resync updates the same row.
    fn migrate_v7_inventory_sources(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sources (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL,
                 config TEXT NOT NULL DEFAULT '{}',
                 created_at TEXT NOT NULL,
                 last_sync_at TEXT,
                 last_sync_error TEXT
             );
             ALTER TABLE connections ADD COLUMN source_id TEXT;
             ALTER TABLE connections ADD COLUMN external_id TEXT;
             ALTER TABLE connections ADD COLUMN stale_since TEXT;",
        )?;
        Ok(())
    }

    fn create_indexes(&self) -> Result<()> {
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_connections_name ON connections(name)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_connections_host ON connections(host)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_connections_last_used ON connections(last_used)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_connection_id ON sessions(connection_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_aliases_connection_id ON aliases(connection_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_exit_code ON sessions(exit_code)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_status_kind ON sessions(status_kind)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_connections_created_at ON connections(created_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_connection_tags_tag ON connection_tags(tag)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_git_root ON sessions(git_root)",
            [],
        )?;
        self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_connections_source_external
                 ON connections(source_id, external_id) WHERE source_id IS NOT NULL",
            [],
        )?;
        // Index on sessions.pid; existing databases already carry it.
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_pid ON sessions(pid)",
            [],
        )?;
        Ok(())
    }

    /// Helper for callers that need a single shared reference to the
    /// connection (e.g. for explicit transactions).
    pub fn raw_connection(&self) -> &SqliteConnection {
        &self.conn
    }

    /// Produce a consistent standalone copy of the database at `dest` using
    /// SQLite's `VACUUM INTO`. Unlike a raw file copy, this is safe while
    /// another process has the source open.
    pub fn vacuum_into(&self, dest: &std::path::Path) -> Result<()> {
        // VACUUM INTO accepts a bound expression, so the path is never
        // spliced into SQL text (and non-UTF-8 paths are rejected instead
        // of being lossily rewritten to a different file name).
        let dest = dest
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("backup path is not valid UTF-8: {}", dest.display()))?;
        self.conn
            .execute("VACUUM INTO ?1", rusqlite::params![dest])?;
        Ok(())
    }

    /// Load tag counts grouped by tag name in a single query (used by stats).
    pub fn tag_counts(&self) -> Result<HashMap<String, usize>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag, COUNT(*) FROM connection_tags GROUP BY tag")?;
        let rows = stmt.query_map([], |r| {
            let tag: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok((tag, count as usize))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (tag, count) = row?;
            map.insert(tag, count);
        }
        Ok(map)
    }
}

/// Escape SQL LIKE wildcards (`%`, `_`, `\`) in a user-supplied string.
/// Pair with `ESCAPE '\'` in the SQL query.
pub(crate) fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{escape_like, Database, SCHEMA_VERSION};
    use crate::config::AppConfig;
    use crate::models::Connection;
    use tempfile::tempdir;

    fn open(dir: &std::path::Path) -> Database {
        let config = AppConfig {
            database_path: dir.join("test.db"),
            ..Default::default()
        };
        Database::new(&config).unwrap()
    }

    fn sample(name: &str) -> Connection {
        Connection::new(
            name.into(),
            "h.example.com".into(),
            "u".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    #[test]
    fn escape_like_doubles_wildcards() {
        assert_eq!(escape_like("foo%bar"), "foo\\%bar");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn close_stale_sessions_ends_only_sessions_whose_process_is_gone() {
        use crate::models::{Session, SessionStatus};

        let dir = tempdir().unwrap();
        let db = open(dir.path());
        let conn = sample("web");
        db.add_connection(&conn).unwrap();

        // A pid that existed and has been reaped, so it is no longer alive.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();

        let mut live = Session::new(conn.clone());
        live.mark_active(std::process::id());
        db.add_session(&live).unwrap();
        let mut dead = Session::new(conn.clone());
        dead.mark_active(dead_pid);
        db.add_session(&dead).unwrap();
        let mut finished = Session::new(conn);
        finished.mark_terminated(0);
        db.add_session(&finished).unwrap();

        let closed = db.close_stale_sessions().unwrap();
        assert_eq!(closed, vec![("web".to_string(), Some(dead_pid))]);

        let history = db.get_session_history(None, 10, None, false).unwrap();
        let open: Vec<_> = history.iter().filter(|e| e.ended_at.is_none()).collect();
        assert_eq!(open.len(), 1, "only the live session stays open");
        assert!(matches!(open[0].status, SessionStatus::Active));
        // Unknown exit: the reconciled session must not be reported as failed.
        assert!(history
            .iter()
            .all(|e| e.ended_at.is_none() || e.exit_code.is_none_or(|c| c == 0)));
    }

    #[test]
    fn removed_tags_do_not_resurrect_after_reopen() {
        let dir = tempdir().unwrap();
        let mut conn = sample("web");
        conn.add_tag("prod".into());
        {
            let db = open(dir.path());
            db.add_connection(&conn).unwrap();
            conn.remove_tag("prod");
            db.update_connection(&conn).unwrap();
            let got = db.get_connection("web").unwrap().unwrap();
            assert!(got.tags.is_empty(), "stale tags returned: {:?}", got.tags);
        }
        let db = open(dir.path());
        let got = db.get_connection("web").unwrap().unwrap();
        assert!(got.tags.is_empty(), "tags resurrected: {:?}", got.tags);
    }

    #[test]
    fn remove_connection_deletes_aliases_and_tags() {
        let dir = tempdir().unwrap();
        let db = open(dir.path());
        let mut conn = sample("web");
        conn.add_tag("prod".into());
        db.add_connection(&conn).unwrap();
        db.add_alias("w", &conn.id.to_string()).unwrap();

        assert!(db.remove_connection("web").unwrap());
        assert!(db.tag_counts().unwrap().is_empty());
        assert!(db.get_connection_by_alias("w").unwrap().is_none());
        let aliases: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM aliases", [], |r| r.get(0))
            .unwrap();
        assert_eq!(aliases, 0);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = tempdir().unwrap();
        {
            let db = open(dir.path());
            db.set_schema_version(SCHEMA_VERSION + 1).unwrap();
        }
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        assert!(Database::new(&config).is_err());
    }
}
