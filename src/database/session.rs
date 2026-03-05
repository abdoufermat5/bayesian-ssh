use crate::database::Database;
use crate::models::Session;
use anyhow::Result;
use rusqlite::params;

impl Database {
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
             WHERE 1=1",
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

            let started_at =
                chrono::DateTime::parse_from_rfc3339(&started_at_str)?.with_timezone(&Utc);
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

    // Active session management
    #[allow(clippy::type_complexity)]
    pub fn get_active_sessions(
        &self,
    ) -> Result<Vec<(String, Option<u32>, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.name, s.pid, s.started_at
             FROM sessions s
             JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status LIKE '%Active%'
             ORDER BY s.started_at DESC",
        )?;
        let mut rows = stmt.query([])?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let started_str: String = row.get(2)?;
            let started_at =
                chrono::DateTime::parse_from_rfc3339(&started_str)?.with_timezone(&chrono::Utc);
            sessions.push((row.get(0)?, row.get(1)?, started_at));
        }
        Ok(sessions)
    }

    #[allow(clippy::type_complexity)]
    pub fn get_active_sessions_for_connection(
        &self,
        target: &str,
    ) -> Result<Vec<(String, String, Option<u32>, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, c.name, s.pid, s.started_at
             FROM sessions s
             JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status LIKE '%Active%'
               AND (c.name LIKE ? OR c.id = ?)
             ORDER BY s.started_at DESC",
        )?;
        let like_pattern = format!("%{}%", target);
        let mut rows = stmt.query(params![like_pattern, target])?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let started_str: String = row.get(3)?;
            let started_at =
                chrono::DateTime::parse_from_rfc3339(&started_str)?.with_timezone(&chrono::Utc);
            sessions.push((row.get(0)?, row.get(1)?, row.get(2)?, started_at));
        }
        Ok(sessions)
    }

    pub fn get_session_id_by_pid(&self, pid: u32) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT id FROM sessions WHERE pid = ? AND ended_at IS NULL",
            params![pid],
            |row| row.get(0),
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
}
