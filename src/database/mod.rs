use crate::config::AppConfig;
use anyhow::Result;
use rusqlite::Connection as SqliteConnection;

pub struct Database {
    pub(crate) conn: SqliteConnection,
}

mod alias;
mod connection;
mod search;
mod session;

impl Database {
    pub fn new(config: &AppConfig) -> Result<Self> {
        // Ensure database directory exists
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = SqliteConnection::open(&config.database_path)?;
        let db = Database { conn };
        db.init()?;

        tracing::debug!("Database initialized at {:?}", config.database_path);
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        // Create connections table
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
                tags TEXT NOT NULL
            )",
            [],
        )?;

        // Create sessions table
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

        // Additive migration: sessions.transport was added in 1.5.0.
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

        // Create indexes
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

        // Create aliases table for connection aliasing
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS aliases (
                alias TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (connection_id) REFERENCES connections (id) ON DELETE CASCADE
            )",
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

        Ok(())
    }
}
