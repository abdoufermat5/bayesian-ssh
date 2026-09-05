use crate::database::Database;
use crate::models::Connection;
use anyhow::{anyhow, Result};
use rusqlite::params;
use tracing::info;

impl Database {
    // ──────────────────────────────────────────────────────────────────────────
    // Connection management
    // ──────────────────────────────────────────────────────────────────────────

    /// Insert or update a connection. Uses an UPSERT keyed on `id` so
    /// importing a backup doesn't accidentally clobber an existing row
    /// that has a different `id` but the same `name`. The tag set is
    /// written transactionally with the connection row.
    pub fn add_connection(&self, connection: &Connection) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        // Detect name conflicts: if a *different* row already owns the
        // name, fail with a typed error rather than silently clobbering.
        let name_clash: Option<String> = tx
            .query_row(
                "SELECT id FROM connections WHERE name = ? AND id != ?",
                params![connection.name, connection.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if let Some(other_id) = name_clash {
            return Err(anyhow!(
                "Duplicate connection name '{}' (existing id: {})",
                connection.name,
                other_id
            ));
        }

        tx.execute(
            "INSERT INTO connections
             (id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                host = excluded.host,
                user = excluded.user,
                port = excluded.port,
                bastion = excluded.bastion,
                bastion_user = excluded.bastion_user,
                use_kerberos = excluded.use_kerberos,
                key_path = excluded.key_path,
                last_used = excluded.last_used",
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
                serde_json::to_string(&connection.tags)?,
            ],
        )?;

        // Replace tag set atomically within the same transaction.
        tx.execute(
            "DELETE FROM connection_tags WHERE connection_id = ?",
            params![connection.id.to_string()],
        )?;
        for tag in &connection.tags {
            if let Some(normalized) = Connection::normalize_tag(tag) {
                tx.execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
                    params![connection.id.to_string(), normalized],
                )?;
            }
        }

        tx.commit()?;
        info!("Connection '{}' added to database", connection.name);
        Ok(())
    }

    pub fn get_connection(&self, name_or_id: &str) -> Result<Option<Connection>> {
        // Single-row path: project all 12 columns (incl. the legacy
        // JSON `tags` column) so `row_to_connection` can populate the
        // in-memory tags directly.
        let mut stmt = self.conn.prepare(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE id = ? OR name = ?",
        )?;

        let mut rows = stmt.query(params![name_or_id, name_or_id])?;

        if let Some(row) = rows.next()? {
            let mut connection = self.row_to_connection(row)?;
            // Prefer the normalized `connection_tags` table; the JSON
            // column may be stale.
            if let Ok(tags) = self.get_tags_for_connection(&connection.id.to_string()) {
                if !tags.is_empty() {
                    connection.tags = tags;
                }
            }
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
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used
             FROM connections",
        );

        let mut conditions = Vec::new();
        let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(tag) = tag_filter {
            // Exact-match against the normalized tags table; no JSON LIKE.
            conditions.push("id IN (SELECT connection_id FROM connection_tags WHERE tag = ?)");
            sql_params.push(Box::new(
                Connection::normalize_tag(tag).ok_or_else(|| anyhow!("invalid tag filter"))?,
            ));
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
            sql_params.iter().map(|p| p.as_ref()),
        ))?;

        let mut connections = Vec::new();
        let mut ids = Vec::new();
        while let Some(row) = rows.next()? {
            let conn = self.row_to_connection(row)?;
            ids.push(conn.id.to_string());
            connections.push(conn);
        }

        // Batch-fetch tags in a single query (avoids N+1).
        let tag_map = self.get_tags_for_connections(&ids)?;
        for conn in &mut connections {
            if let Some(tags) = tag_map.get(&conn.id.to_string()) {
                conn.tags = tags.clone();
            }
        }

        // Batch-fetch aliases in a single query as well.
        let alias_map = self.get_aliases_for_connections(&ids)?;
        for conn in &mut connections {
            if let Some(aliases) = alias_map.get(&conn.id.to_string()) {
                conn.aliases = aliases.clone();
            }
        }

        Ok(connections)
    }

    pub fn update_connection(&self, connection: &Connection) -> Result<()> {
        // Don't allow renaming onto an existing name.
        let clash: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM connections WHERE name = ? AND id != ?",
                params![connection.name, connection.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if let Some(other_id) = clash {
            return Err(anyhow!(
                "Cannot rename: name '{}' is already used by connection {}",
                connection.name,
                other_id
            ));
        }

        let tx = self.conn.unchecked_transaction()?;
        let rows = tx.execute(
            "UPDATE connections SET
             name = ?, host = ?, user = ?, port = ?, bastion = ?, bastion_user = ?,
             use_kerberos = ?, key_path = ?, last_used = ?
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
                connection.id.to_string(),
            ],
        )?;
        if rows == 0 {
            return Err(anyhow!(
                "Connection '{}' was not found or could not be updated",
                connection.name
            ));
        }

        // Re-sync the tag set inside the same transaction.
        tx.execute(
            "DELETE FROM connection_tags WHERE connection_id = ?",
            params![connection.id.to_string()],
        )?;
        for tag in &connection.tags {
            if let Some(normalized) = Connection::normalize_tag(tag) {
                tx.execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
                    params![connection.id.to_string(), normalized],
                )?;
            }
        }
        tx.commit()?;

        info!("Connection '{}' updated in database", connection.name);
        Ok(())
    }

    /// Best-effort "touch" of a connection's `last_used` timestamp.
    ///
    /// Unlike [`Database::update_connection`] this does NOT error when the
    /// row has been concurrently deleted (which would otherwise fail a
    /// connect that raced with a `bssh remove`). This is the correct
    /// semantics for the connect path, where updating recency is a nice
    /// to have but not a hard requirement.
    pub fn touch_connection(&self, connection: &Connection) -> Result<()> {
        self.conn.execute(
            "UPDATE connections SET last_used = ? WHERE id = ?",
            params![
                connection.last_used.map(|d| d.to_rfc3339()),
                connection.id.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn remove_connection(&self, name_or_id: &str) -> Result<bool> {
        let connection_id = if let Some(conn) = self.get_connection(name_or_id)? {
            conn.id.to_string()
        } else {
            return Ok(false);
        };

        // Wrap both deletes in a single transaction so a crash mid-way
        // can't leave orphan sessions.
        let tx = self.conn.unchecked_transaction()?;
        let sessions_deleted = tx.execute(
            "DELETE FROM sessions WHERE connection_id = ?",
            params![connection_id],
        )?;
        let rows_affected = tx.execute(
            "DELETE FROM connections WHERE id = ?",
            params![connection_id],
        )?;
        tx.commit()?;

        info!(
            "Deleted {} sessions and connection '{}'",
            sessions_deleted, name_or_id
        );
        Ok(rows_affected > 0)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Helper methods
    // ──────────────────────────────────────────────────────────────────────────

    /// Convert a single row into a `Connection`. Tags are NOT read here;
    /// callers should fetch them in a single batch query and assign them
    /// to the returned `Connection.tags` (see `get_tags_for_connections`).
    pub(crate) fn row_to_connection(&self, row: &rusqlite::Row) -> Result<Connection> {
        let id_str: String = row.get(0)?;
        let id = uuid::Uuid::parse_str(&id_str)?;

        // Read tags from the legacy JSON column if the SELECT projects
        // it (column index 11). New query paths omit it and use the
        // normalized `connection_tags` table instead.
        let tags: Vec<String> = if row.as_ref().column_count() > 11 {
            let tags_json: Option<String> = row.get(11)?;
            tags_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Connection {
            id,
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
            last_used: row.get::<_, Option<String>>(10)?.and_then(|s| {
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

        // Helper to map any anyhow error from `row_to_connection` to a
        // generic SQL conversion error (rusqlite's closures must
        // return `Result<_, rusqlite::Error>`).
        let map_err_rusqlite = |e: anyhow::Error| -> rusqlite::Error {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::other(e.to_string())),
            )
        };

        // Single query for most-recently-used.
        let most_used = self
            .conn
            .query_row(
                "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used
                 FROM connections
                 WHERE last_used IS NOT NULL
                 ORDER BY last_used DESC
                 LIMIT 1",
                [],
                |row| self.row_to_connection(row).map_err(map_err_rusqlite),
            )
            .ok();

        // Single query for the ten most-recently-used.
        let mut recent_stmt = self.conn.prepare(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used
             FROM connections
             WHERE last_used IS NOT NULL
             ORDER BY last_used DESC
             LIMIT 10",
        )?;
        let recent_rows = recent_stmt.query_map([], |row| {
            self.row_to_connection(row).map_err(map_err_rusqlite)
        })?;
        let mut recent_connections: Vec<Connection> = recent_rows.filter_map(Result::ok).collect();

        // Batch-fetch tags for all "stats" connections in one query.
        let mut all_ids: Vec<String> = recent_connections
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        if let Some(ref m) = most_used {
            let id = m.id.to_string();
            if !all_ids.contains(&id) {
                all_ids.push(id);
            }
        }
        let tag_map = self.get_tags_for_connections(&all_ids)?;
        for conn in &mut recent_connections {
            if let Some(tags) = tag_map.get(&conn.id.to_string()) {
                conn.tags = tags.clone();
            }
        }

        // Tag counts from a single GROUP BY (no full table scan in Rust).
        let tag_counts = self.tag_counts()?;

        let most_used = most_used.map(|mut c| {
            if let Some(tags) = tag_map.get(&c.id.to_string()) {
                c.tags = tags.clone();
            }
            c
        });

        Ok(crate::models::ConnectionStats {
            total_connections: total_connections as usize,
            most_used,
            recently_used: recent_connections,
            by_tag: tag_counts,
        })
    }
}
