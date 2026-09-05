use crate::config::AppConfig;
use anyhow::Result;
use rusqlite::Connection as SqliteConnection;
use std::collections::HashMap;

pub struct Database {
    pub(crate) conn: SqliteConnection,
}

mod alias;
mod connection;
mod search;
mod session;
mod tag;

pub use search::SearchField;

const SCHEMA_VERSION: i32 = 3;

impl Database {
    pub fn new(config: &AppConfig) -> Result<Self> {
        // Ensure database directory exists
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)?;
            crate::config::enforce_secure_dir(parent);
        }

        let conn = SqliteConnection::open(&config.database_path)?;
        crate::config::enforce_secure_file(&config.database_path);

        let db = Database { conn };
        db.init()?;

        tracing::debug!("Database initialized at {:?}", config.database_path);
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        // Use a single transaction for the full migration so the schema is
        // always either fully applied or fully rolled back.
        self.conn.execute_batch("BEGIN")?;
        let result: Result<()> = (|| {
            self.apply_migrations()?;
            self.create_indexes()?;
            self.backfill_tags_table()?;
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
        debug_assert_eq!(self.current_schema_version()?, SCHEMA_VERSION);
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
        Ok(())
    }

    fn backfill_tags_table(&self) -> Result<()> {
        // Only run if the JSON column still has data we haven't migrated.
        // We rely on the fact that this is called once per DB open; doing
        // it every time is cheap (empty result after first run).
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
            for tag in tags {
                self.conn.execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
                    rusqlite::params![id, tag],
                )?;
            }
        }
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
        Ok(())
    }

    /// Helper for callers that need a single shared reference to the
    /// connection (e.g. for explicit transactions).
    pub fn raw_connection(&self) -> &SqliteConnection {
        &self.conn
    }

    /// Produce a consistent standalone copy of the database at `dest` using
    /// SQLite's `VACUUM INTO`. Unlike a raw file copy, this is safe while
    /// the source is open in WAL mode.
    pub fn vacuum_into(&self, dest: &std::path::Path) -> Result<()> {
        self.conn.execute_batch(&format!(
            "VACUUM INTO '{}'",
            dest.display().to_string().replace('\'', "''")
        ))?;
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
