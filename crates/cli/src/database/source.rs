use crate::database::Database;
use crate::models::{Connection, Source, SourceKind};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

/// A source plus the counts shown by `bssh source list`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceSummary {
    #[serde(flatten)]
    pub source: Source,
    pub hosts: usize,
    pub stale: usize,
}

impl Database {
    pub fn add_source(&self, source: &Source) -> Result<()> {
        let config = serde_json::to_string(&source.config)?;
        self.conn.execute(
            "INSERT INTO sources (id, name, kind, config, created_at, last_sync_at, last_sync_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                source.id.to_string(),
                source.name,
                source.kind.as_str(),
                config,
                source.created_at.to_rfc3339(),
                source.last_sync_at.map(|t| t.to_rfc3339()),
                source.last_sync_error,
            ],
        )?;
        Ok(())
    }

    pub fn get_source(&self, name_or_id: &str) -> Result<Option<Source>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, config, created_at, last_sync_at, last_sync_error
             FROM sources WHERE id = ?1 OR name = ?1",
        )?;
        let source = stmt
            .query_row(params![name_or_id], |row| {
                let kind: String = row.get(2)?;
                Ok(Source {
                    id: parse_uuid(&row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    kind: kind.parse().map_err(|_| {
                        rusqlite::Error::InvalidColumnType(
                            2,
                            "kind".into(),
                            rusqlite::types::Type::Text,
                        )
                    })?,
                    config: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                    created_at: parse_time(&row.get::<_, String>(4)?),
                    last_sync_at: row.get::<_, Option<String>>(5)?.map(|t| parse_time(&t)),
                    last_sync_error: row.get(6)?,
                })
            })
            .optional()?;
        Ok(source)
    }

    pub fn list_sources(&self) -> Result<Vec<SourceSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.kind, s.config, s.created_at, s.last_sync_at, s.last_sync_error,
                    COUNT(c.id),
                    COUNT(c.stale_since)
             FROM sources s
             LEFT JOIN connections c ON c.source_id = s.id
             GROUP BY s.id
             ORDER BY s.name ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let kind: String = row.get(2)?;
                Ok(SourceSummary {
                    source: Source {
                        id: parse_uuid(&row.get::<_, String>(0)?),
                        name: row.get(1)?,
                        kind: kind.parse().unwrap_or(SourceKind::SshConfig),
                        config: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                        created_at: parse_time(&row.get::<_, String>(4)?),
                        last_sync_at: row.get::<_, Option<String>>(5)?.map(|t| parse_time(&t)),
                        last_sync_error: row.get(6)?,
                    },
                    hosts: row.get::<_, i64>(7)? as usize,
                    stale: row.get::<_, i64>(8)? as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Remove a source. By default its connections are kept and detached; with
    /// `delete_hosts` they are removed with the same cleanup as
    /// `remove_connection`.
    pub fn remove_source(&self, id: Uuid, delete_hosts: bool) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        if delete_hosts {
            let host_ids: Vec<String> = {
                let mut stmt = tx.prepare("SELECT id FROM connections WHERE source_id = ?1")?;
                let rows = stmt.query_map(params![id.to_string()], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for host_id in host_ids {
                tx.execute(
                    "DELETE FROM sessions WHERE connection_id = ?1",
                    params![host_id],
                )?;
                tx.execute(
                    "DELETE FROM aliases WHERE connection_id = ?1",
                    params![host_id],
                )?;
                tx.execute(
                    "DELETE FROM connection_tags WHERE connection_id = ?1",
                    params![host_id],
                )?;
                tx.execute("DELETE FROM connections WHERE id = ?1", params![host_id])?;
            }
        } else {
            tx.execute(
                "UPDATE connections SET source_id = NULL, external_id = NULL, stale_since = NULL
                 WHERE source_id = ?1",
                params![id.to_string()],
            )?;
        }

        tx.execute("DELETE FROM sources WHERE id = ?1", params![id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// Store the outcome of a sync: the timestamp on success, the error message
    /// on failure. The previous timestamp is kept when a sync fails.
    pub fn record_sync_result(
        &self,
        id: Uuid,
        at: DateTime<Utc>,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET last_sync_at = ?2, last_sync_error = ?3 WHERE id = ?1",
            params![id.to_string(), at.to_rfc3339(), error],
        )?;
        Ok(())
    }

    pub fn connections_for_source(&self, id: Uuid) -> Result<Vec<Connection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path,
                    created_at, last_used, source_id, external_id, stale_since
             FROM connections WHERE source_id = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![id.to_string()], |row| {
            self.row_to_connection(row).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    error.into(),
                )
            })
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        let mut connections = Vec::with_capacity(rows.len());
        for mut connection in rows {
            connection.tags = self.get_tags_for_connection(&connection.id.to_string())?;
            connections.push(connection);
        }
        Ok(connections)
    }
}

fn parse_uuid(value: &str) -> Uuid {
    // Rows are written by us; a corrupt id should not take the whole list
    // down, so fall back to a nil id that simply matches nothing.
    Uuid::parse_str(value).unwrap_or(Uuid::nil())
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
