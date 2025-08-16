use crate::config::AppConfig;
use crate::models::{Connection, Session};
use anyhow::Result;
use rusqlite::{params, Connection as SqliteConnection};
use tracing::info;

pub struct Database {
    conn: SqliteConnection,
}

impl Database {
    pub fn new(config: &AppConfig) -> Result<Self> {
        // Ensure database directory exists
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = SqliteConnection::open(&config.database_path)?;
        let db = Database { conn };
        db.init()?;

        info!("Database initialized at {:?}", config.database_path);
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
                FOREIGN KEY (connection_id) REFERENCES connections (id)
            )",
            [],
        )?;

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

        Ok(())
    }

    // Connection management
    pub fn add_connection(&self, connection: &Connection) -> Result<()> {
        let tags_json = serde_json::to_string(&connection.tags)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO connections 
             (id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                connection.id.to_string(),
                connection.name,
                connection.host,
                connection.user,
                connection.port,
                connection.bastion,
                connection.bastion_user,
                connection.use_kerberos,
                connection.key_path,
                connection.created_at.to_rfc3339(),
                connection.last_used.map(|d| d.to_rfc3339()),
                tags_json,
            ],
        )?;

        info!("Connection '{}' added to database", connection.name);
        Ok(())
    }

    pub fn get_connection(&self, name_or_id: &str) -> Result<Option<Connection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections 
             WHERE id = ? OR name = ?"
        )?;

        let mut rows = stmt.query(params![name_or_id, name_or_id])?;

        if let Some(row) = rows.next()? {
            let connection = self.row_to_connection(row)?;
            Ok(Some(connection))
        } else {
            Ok(None)
        }
    }

    pub fn list_connections(
        &self,
        tag_filter: Option<&str>,
        recent_only: bool,
    ) -> Result<Vec<Connection>> {
        let mut query = String::from(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections"
        );

        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(tag) = tag_filter {
            conditions.push("tags LIKE ?");
            params.push(Box::new(format!("%\"{}\"%", tag)));
        }

        if recent_only {
            conditions.push("last_used IS NOT NULL");
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY last_used DESC NULLS LAST, name ASC");

        let mut stmt = self.conn.prepare(&query)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            params.iter().map(|p| p.as_ref()),
        ))?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }

        Ok(connections)
    }

    pub fn update_connection(&self, connection: &Connection) -> Result<()> {
        let tags_json = serde_json::to_string(&connection.tags)?;

        self.conn.execute(
            "UPDATE connections SET 
             name = ?, host = ?, user = ?, port = ?, bastion = ?, bastion_user = ?, 
             use_kerberos = ?, key_path = ?, last_used = ?, tags = ?
             WHERE id = ?",
            params![
                connection.name,
                connection.host,
                connection.user,
                connection.port,
                connection.bastion,
                connection.bastion_user,
                connection.use_kerberos,
                connection.key_path,
                connection.last_used.map(|d| d.to_rfc3339()),
                tags_json,
                connection.id.to_string(),
            ],
        )?;

        info!("Connection '{}' updated in database", connection.name);
        Ok(())
    }

    pub fn remove_connection(&self, name_or_id: &str) -> Result<bool> {
        // First, find the connection to get its ID
        let connection_id = if let Some(conn) = self.get_connection(name_or_id)? {
            conn.id.to_string()
        } else {
            return Ok(false); // Connection not found
        };

        // Delete all sessions for this connection first
        let mut stmt = self
            .conn
            .prepare("DELETE FROM sessions WHERE connection_id = ?")?;
        let sessions_deleted = stmt.execute(params![connection_id])?;
        info!(
            "Deleted {} sessions for connection '{}'",
            sessions_deleted, name_or_id
        );

        // Now delete the connection
        let mut stmt = self
            .conn
            .prepare("DELETE FROM connections WHERE id = ? OR name = ?")?;
        let rows_affected = stmt.execute(params![name_or_id, name_or_id])?;

        if rows_affected > 0 {
            info!("Connection '{}' removed from database", name_or_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // Session management
    pub fn add_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, connection_id, started_at, ended_at, status, pid, exit_code)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                session.id.to_string(),
                session.connection.id.to_string(),
                session.started_at.to_rfc3339(),
                session.ended_at.map(|d| d.to_rfc3339()),
                serde_json::to_string(&session.status)?,
                session.pid,
                session.exit_code,
            ],
        )?;

        Ok(())
    }

    pub fn update_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET 
             ended_at = ?, status = ?, pid = ?, exit_code = ?
             WHERE id = ?",
            params![
                session.ended_at.map(|d| d.to_rfc3339()),
                serde_json::to_string(&session.status)?,
                session.pid,
                session.exit_code,
                session.id.to_string(),
            ],
        )?;

        Ok(())
    }

    // Helper methods
    fn row_to_connection(&self, row: &rusqlite::Row) -> Result<Connection> {
        let tags_json: String = row.get(11)?;
        let tags: Vec<String> = serde_json::from_str(&tags_json)?;

        Ok(Connection {
            id: uuid::Uuid::parse_str(&row.get::<_, String>(0)?)?,
            name: row.get(1)?,
            host: row.get(2)?,
            user: row.get(3)?,
            port: row.get(4)?,
            bastion: row.get(5)?,
            bastion_user: row.get(6)?,
            use_kerberos: row.get(7)?,
            key_path: row.get(8)?,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)?
                .with_timezone(&chrono::Utc),
            last_used: row.get::<_, Option<String>>(10)?.map(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            }),
            tags,
        })
    }

    pub fn get_stats(&self) -> Result<crate::models::ConnectionStats> {
        let total_connections: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM connections", [], |row| row.get(0))?;

        let most_used = {
            let result = self.conn.query_row(
                "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
                 FROM connections 
                 WHERE last_used IS NOT NULL 
                 ORDER BY last_used DESC 
                 LIMIT 1",
                [],
                |row| {
                    let conn_result = self.row_to_connection(row);
                    match conn_result {
                        Ok(conn) => Ok(conn),
                        Err(_) => Err(rusqlite::Error::InvalidParameterName("Failed to parse connection".to_string())),
                    }
                }
            );
            match result {
                Ok(conn) => Some(conn),
                Err(_) => None,
            }
        };

        let mut recent_connections = self.list_connections(None, true)?;
        recent_connections.truncate(10);

        let mut tag_counts = std::collections::HashMap::new();
        let connections = self.list_connections(None, false)?;
        for conn in connections {
            for tag in &conn.tags {
                *tag_counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }

        Ok(crate::models::ConnectionStats {
            total_connections: total_connections as usize,
            most_used,
            recently_used: recent_connections,
            by_tag: tag_counts,
        })
    }
}
