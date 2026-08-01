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

    /// Batch-fetch aliases for multiple connection IDs in a single query.
    /// Returns a map keyed by connection_id (as stored in the DB, i.e. string form).
    pub(crate) fn get_aliases_for_connections(
        &self,
        connection_ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut map = std::collections::HashMap::new();
        if connection_ids.is_empty() {
            return Ok(map);
        }
        let placeholders = vec!["?"; connection_ids.len()].join(",");
        let sql = format!(
            "SELECT connection_id, alias FROM aliases WHERE connection_id IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            connection_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let mut rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
        while let Some(row) = rows.next()? {
            let conn_id: String = row.get(0)?;
            let alias: String = row.get(1)?;
            map.entry(conn_id).or_insert_with(Vec::new).push(alias);
        }
        Ok(map)
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

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;
    use crate::database::Database;
    use crate::models::Connection;
    use tempfile::tempdir;

    #[test]
    fn test_batch_get_aliases_for_connections() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        let conn1 = Connection::new(
            "web-prod".into(),
            "web1.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        let conn2 = Connection::new(
            "db-prod".into(),
            "db1.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        db.add_connection(&conn1).unwrap();
        db.add_connection(&conn2).unwrap();

        db.add_alias("wp", &conn1.id.to_string()).unwrap();
        db.add_alias("webp", &conn1.id.to_string()).unwrap();
        db.add_alias("dbp", &conn2.id.to_string()).unwrap();

        let ids = vec![conn1.id.to_string(), conn2.id.to_string()];
        let map = db.get_aliases_for_connections(&ids).unwrap();

        let mut a1 = map.get(&conn1.id.to_string()).unwrap().clone();
        a1.sort();
        assert_eq!(a1, vec!["webp", "wp"]);

        let a2 = map.get(&conn2.id.to_string()).unwrap();
        assert_eq!(a2, &vec!["dbp"]);
    }

    #[test]
    fn test_batch_get_aliases_empty() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();
        let map = db.get_aliases_for_connections(&[]).unwrap();
        assert!(map.is_empty());
    }
}
