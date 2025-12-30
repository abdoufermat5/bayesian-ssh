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

    // Session history retrieval
    pub fn get_session_history(
        &self,
        connection_filter: Option<&str>,
        limit: usize,
        days: Option<u32>,
        show_failed_only: bool,
    ) -> Result<Vec<crate::models::SessionHistoryEntry>> {
        use crate::models::{SessionHistoryEntry, SessionStatus};
        use chrono::{Duration, Utc};

        let mut query = String::from(
            "SELECT s.id, c.name, s.started_at, s.ended_at, s.status, s.exit_code
             FROM sessions s
             JOIN connections c ON s.connection_id = c.id
             WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(conn_name) = connection_filter {
            query.push_str(" AND (c.name LIKE ? OR c.id = ?)");
            params.push(Box::new(format!("%{}%", conn_name)));
            params.push(Box::new(conn_name.to_string()));
        }

        if let Some(d) = days {
            let cutoff = Utc::now() - Duration::days(d as i64);
            query.push_str(" AND s.started_at >= ?");
            params.push(Box::new(cutoff.to_rfc3339()));
        }

        if show_failed_only {
            query.push_str(" AND (s.status LIKE '%Error%' OR s.exit_code != 0)");
        }

        query.push_str(" ORDER BY s.started_at DESC LIMIT ?");
        params.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&query)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            params.iter().map(|p| p.as_ref()),
        ))?;

        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let started_at_str: String = row.get(2)?;
            let ended_at_str: Option<String> = row.get(3)?;
            let status_json: String = row.get(4)?;

            let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)?
                .with_timezone(&Utc);
            let ended_at = ended_at_str.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });

            let status: SessionStatus = serde_json::from_str(&status_json)
                .unwrap_or(SessionStatus::Error("unknown".to_string()));

            let duration = ended_at.map(|end| end - started_at);

            entries.push(SessionHistoryEntry {
                connection_name: row.get(1)?,
                started_at,
                ended_at,
                status,
                exit_code: row.get(5)?,
                duration,
            });
        }

        Ok(entries)
    }

    // Alias management
    pub fn add_alias(&self, alias: &str, connection_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO aliases (alias, connection_id, created_at)
             VALUES (?, ?, ?)",
            params![alias, connection_id, chrono::Utc::now().to_rfc3339()],
        )?;
        info!("Alias '{}' added for connection {}", alias, connection_id);
        Ok(())
    }

    pub fn remove_alias(&self, alias: &str) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM aliases WHERE alias = ?",
            params![alias],
        )?;
        Ok(rows > 0)
    }

    pub fn get_aliases_for_connection(&self, connection_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT alias FROM aliases WHERE connection_id = ?"
        )?;
        let mut rows = stmt.query(params![connection_id])?;
        
        let mut aliases = Vec::new();
        while let Some(row) = rows.next()? {
            aliases.push(row.get(0)?);
        }
        Ok(aliases)
    }

    pub fn get_connection_by_alias(&self, alias: &str) -> Result<Option<Connection>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.host, c.user, c.port, c.bastion, c.bastion_user, 
                    c.use_kerberos, c.key_path, c.created_at, c.last_used, c.tags
             FROM connections c
             JOIN aliases a ON c.id = a.connection_id
             WHERE a.alias = ?"
        )?;
        let mut rows = stmt.query(params![alias])?;

        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_connection(row)?))
        } else {
            Ok(None)
        }
    }

    /// Enhanced get_connection that also checks aliases
    pub fn get_connection_or_alias(&self, name_or_alias: &str) -> Result<Option<Connection>> {
        // First try direct lookup
        if let Some(conn) = self.get_connection(name_or_alias)? {
            return Ok(Some(conn));
        }
        // Then try alias lookup
        self.get_connection_by_alias(name_or_alias)
    }

    // Active session management
    pub fn get_active_sessions(&self) -> Result<Vec<(String, Option<u32>, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.name, s.pid, s.started_at
             FROM sessions s
             JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status LIKE '%Active%'
             ORDER BY s.started_at DESC"
        )?;
        let mut rows = stmt.query([])?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let started_str: String = row.get(2)?;
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_str)?
                .with_timezone(&chrono::Utc);
            sessions.push((row.get(0)?, row.get(1)?, started_at));
        }
        Ok(sessions)
    }

    pub fn get_active_sessions_for_connection(&self, target: &str) -> Result<Vec<(String, String, Option<u32>, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, c.name, s.pid, s.started_at
             FROM sessions s
             JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status LIKE '%Active%'
               AND (c.name LIKE ? OR c.id = ?)
             ORDER BY s.started_at DESC"
        )?;
        let like_pattern = format!("%{}%", target);
        let mut rows = stmt.query(params![like_pattern, target])?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let started_str: String = row.get(3)?;
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_str)?
                .with_timezone(&chrono::Utc);
            sessions.push((row.get(0)?, row.get(1)?, row.get(2)?, started_at));
        }
        Ok(sessions)
    }

    pub fn get_session_id_by_pid(&self, pid: u32) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT id FROM sessions WHERE pid = ? AND ended_at IS NULL",
            params![pid],
            |row| row.get(0)
        );
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn mark_session_terminated(&self, session_id: &str, exit_code: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?, status = ?, exit_code = ? WHERE id = ?",
            params![
                chrono::Utc::now().to_rfc3339(),
                "\"Terminated\"",
                exit_code,
                session_id
            ],
        )?;
        Ok(())
    }

    pub fn mark_all_sessions_terminated(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?, status = ?, exit_code = -1 WHERE ended_at IS NULL",
            params![chrono::Utc::now().to_rfc3339(), "\"Terminated\""],
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
            aliases: Vec::new(), // Loaded separately when needed
            created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)?
                .with_timezone(&chrono::Utc),
            last_used: row
                .get::<_, Option<String>>(10)?
                .and_then(|s| {
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&chrono::Utc))
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
            result.ok()
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

    // Fuzzy search methods for enhanced connection discovery
    pub fn fuzzy_search_connections(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let mut all_matches = Vec::new();
        let normalized_query = query.to_lowercase();

        // Search in names with multiple strategies
        if let Ok(mut name_matches) = self.search_by_field(&normalized_query, "name", limit) {
            all_matches.append(&mut name_matches);
        }

        // Enhanced fuzzy matching for names
        if let Ok(mut fuzzy_matches) = self.enhanced_fuzzy_search(&normalized_query, limit) {
            all_matches.append(&mut fuzzy_matches);
        }

        // Search in hosts
        if let Ok(mut host_matches) = self.search_by_field(&normalized_query, "host", limit) {
            all_matches.append(&mut host_matches);
        }

        // Search in tags (JSON array search)
        if let Ok(mut tag_matches) = self.search_in_tags(&normalized_query, limit) {
            all_matches.append(&mut tag_matches);
        }

        // Remove duplicates and sort by relevance
        self.deduplicate_and_rank(&mut all_matches, &normalized_query);

        // Limit results
        all_matches.truncate(limit);

        Ok(all_matches)
    }

    fn search_by_field(&self, query: &str, field: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = format!(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE {} LIKE ? COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?",
            field
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let like_pattern = format!("%{}%", query);
        let mut rows = stmt.query(params![like_pattern, limit])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }

        Ok(connections)
    }

    fn enhanced_fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
                   FROM connections
                   ORDER BY last_used DESC NULLS LAST, name ASC";

        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            let connection = self.row_to_connection(row)?;
            let name_lower = connection.name.to_lowercase();

            // Enhanced matching patterns
            if self.matches_enhanced_patterns(query, &name_lower) {
                connections.push(connection);
                if connections.len() >= limit {
                    break;
                }
            }
        }

        Ok(connections)
    }

    fn matches_enhanced_patterns(&self, query: &str, name: &str) -> bool {
        let query = query.to_lowercase();

        // 1. Standard substring match (already covered by search_by_field)

        // 2. Word-based matching - split query into words and find them
        let query_words: Vec<&str> = query.split_whitespace().collect();
        if query_words.len() > 1 {
            let all_words_found = query_words.iter().all(|word| name.contains(word));
            if all_words_found {
                return true;
            }
        }

        // 3. Handle common separators (hyphens, underscores, dots)
        let normalized_name = name.replace("-", "").replace("_", "").replace(".", "");

        let normalized_query = query.replace("-", "").replace("_", "").replace(".", "");

        // Check if normalized versions match
        if normalized_name.contains(&normalized_query) {
            return true;
        }

        // 4. Acronym matching (first letters of words)
        if query.len() >= 2 {
            let words: Vec<&str> = name.split(&['-', '_', ' '][..]).collect();
            if words.len() > 1 {
                let acronym: String = words
                    .iter()
                    .filter_map(|word| word.chars().next())
                    .collect();
                if acronym.to_lowercase().contains(&query) {
                    return true;
                }
            }
        }

        // 5. Partial acronym matching
        if query.len() >= 2 {
            let name_chars: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            if name_chars.to_lowercase().starts_with(&query) {
                return true;
            }
        }

        false
    }

    fn search_in_tags(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql =
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE tags LIKE ? COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?";

        let mut stmt = self.conn.prepare(sql)?;
        let like_pattern = format!("%\"{}\"%", query);
        let mut rows = stmt.query(params![like_pattern, limit])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }

        Ok(connections)
    }

    fn deduplicate_and_rank(&self, connections: &mut Vec<Connection>, query: &str) {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        connections.retain(|conn| {
            if seen.contains(&conn.id) {
                false
            } else {
                seen.insert(conn.id);
                true
            }
        });

        // Sort by relevance score
        connections.sort_by(|a, b| {
            let score_a = self.calculate_relevance_score(a, query);
            let score_b = self.calculate_relevance_score(b, query);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    fn calculate_relevance_score(&self, connection: &Connection, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let name_lower = connection.name.to_lowercase();
        let mut score = 0.0;

        // Exact match in name gets highest score
        if name_lower == query_lower {
            score += 100.0;
        }

        // Starts with query (high relevance)
        if name_lower.starts_with(&query_lower) {
            score += 50.0;
        }

        // Contains query in name
        if name_lower.contains(&query_lower) {
            score += 25.0;
        }

        // Enhanced pattern matching scores
        if self.matches_enhanced_patterns(query, &name_lower) {
            score += 15.0; // Bonus for pattern matching
        }

        // Query in host
        if connection.host.to_lowercase().contains(&query_lower) {
            score += 15.0;
        }

        // Query in tags
        for tag in &connection.tags {
            if tag.to_lowercase().contains(&query_lower) {
                score += 20.0;
                break;
            }
        }

        // Recent usage bonus
        if let Some(last_used) = connection.last_used {
            let hours_since_used = chrono::Utc::now()
                .signed_duration_since(last_used)
                .num_hours();

            if hours_since_used < 24 {
                score += 30.0;
            } else if hours_since_used < 168 {
                // 1 week
                score += 15.0;
            } else if hours_since_used < 720 {
                // 1 month
                score += 5.0;
            }
        }

        score
    }
}
