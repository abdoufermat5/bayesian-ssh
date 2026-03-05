use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use rusqlite::params;
use tracing::info;

impl Database {
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
        let rows = self
            .conn
            .execute("DELETE FROM aliases WHERE alias = ?", params![alias])?;
        Ok(rows > 0)
    }

    pub fn get_aliases_for_connection(&self, connection_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT alias FROM aliases WHERE connection_id = ?")?;
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
             WHERE a.alias = ?",
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
}
