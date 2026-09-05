use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use rusqlite::params;
use tracing::info;

impl Database {
    /// Replace the entire tag set for a connection.
    /// Runs in a transaction so the connection's tag list is always coherent.
    pub fn set_connection_tags(&self, connection_id: &str, tags: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM connection_tags WHERE connection_id = ?",
            params![connection_id],
        )?;
        for tag in tags {
            if let Some(normalized) = Connection::normalize_tag(tag) {
                tx.execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
                    params![connection_id, normalized],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_tag(&self, connection_id: &str, tag: &str) -> Result<()> {
        let Some(normalized) = Connection::normalize_tag(tag) else {
            return Ok(());
        };
        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
            params![connection_id, normalized],
        )?;
        if rows > 0 {
            info!("Tag '{}' added to connection {}", normalized, connection_id);
        }
        Ok(())
    }

    pub fn remove_tag(&self, connection_id: &str, tag: &str) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM connection_tags WHERE connection_id = ? AND tag = ?",
            params![connection_id, tag],
        )?;
        Ok(rows > 0)
    }

    /// Fetch the tag set for a single connection. Used by `row_to_connection`
    /// to populate the in-memory `tags` field; for batch loads, prefer
    /// `get_tags_for_connections`.
    pub fn get_tags_for_connection(&self, connection_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM connection_tags WHERE connection_id = ? ORDER BY tag")?;
        let mut rows = stmt.query(params![connection_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        Ok(out)
    }

    /// Batch-fetch tags for many connection IDs in a single query.
    /// Returns a map keyed by connection_id.
    pub(crate) fn get_tags_for_connections(
        &self,
        connection_ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if connection_ids.is_empty() {
            return Ok(map);
        }
        let placeholders = vec!["?"; connection_ids.len()].join(",");
        let sql = format!(
            "SELECT connection_id, tag FROM connection_tags
             WHERE connection_id IN ({}) ORDER BY connection_id, tag",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = connection_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let mut rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
        while let Some(row) = rows.next()? {
            let conn_id: String = row.get(0)?;
            let tag: String = row.get(1)?;
            map.entry(conn_id).or_default().push(tag);
        }
        Ok(map)
    }
}
