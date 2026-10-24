use crate::database::{escape_like, Database};
use crate::models::{Session, SessionStatusKind, SessionTextHit};
use anyhow::Result;
use rusqlite::params;
use uuid::Uuid;

impl Database {
    // ──────────────────────────────────────────────────────────────────────────
    // Session management
    // ──────────────────────────────────────────────────────────────────────────

    pub fn add_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            // `connection_id` is set only when the host is saved: a direct
            // connection is recorded by its target alone.
            "INSERT INTO sessions (id, connection_id, target, started_at, ended_at, status, status_kind, pid, exit_code,
                                   transport, recording_path, cwd, git_root, prev_connection_id, local_hour, local_weekday)
             VALUES (?, (SELECT id FROM connections WHERE id = ?), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                session.id.to_string(),
                session.connection.id.to_string(),
                session.target(),
                session.started_at.to_rfc3339(),
                session.ended_at.map(|d| d.to_rfc3339()),
                serde_json::to_string(&session.status)?,
                session.status.kind().as_str(),
                session.pid,
                session.exit_code,
                session.transport.as_deref(),
                session.recording_path.as_deref(),
                session.context.cwd.as_deref(),
                session.context.git_root.as_deref(),
                session.context.prev_connection_id.map(|id| id.to_string()),
                session.context.local_hour,
                session.context.local_weekday,
            ],
        )?;
        Ok(())
    }

    pub fn update_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET
             ended_at = ?, status = ?, status_kind = ?, pid = ?, exit_code = ?, transport = ?,
             recording_path = ?
             WHERE id = ?",
            params![
                session.ended_at.map(|d| d.to_rfc3339()),
                serde_json::to_string(&session.status)?,
                session.status.kind().as_str(),
                session.pid,
                session.exit_code,
                session.transport.as_deref(),
                session.recording_path.as_deref(),
                session.id.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Return the connection ID of the most recent session started at or after `cutoff`.
    pub fn last_connection_since(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<uuid::Uuid>> {
        use rusqlite::OptionalExtension;
        let conn_id_str: Option<String> = self
            .conn
            .query_row(
                "SELECT connection_id FROM sessions
                 WHERE started_at >= ? AND connection_id IS NOT NULL
                 ORDER BY started_at DESC
                 LIMIT 1",
                params![cutoff.to_rfc3339()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(conn_id_str.and_then(|s| uuid::Uuid::parse_str(&s).ok()))
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Session history retrieval
    // ──────────────────────────────────────────────────────────────────────────

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
            "SELECT s.id, COALESCE(c.name, s.target, '?'), s.started_at, s.ended_at, s.status, s.exit_code,
                    s.recording_path
             FROM sessions s
             LEFT JOIN connections c ON s.connection_id = c.id
             WHERE 1=1",
        );
        let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(conn_name) = connection_filter {
            // Escape LIKE wildcards in the user-supplied filter; compare
            // by id (exact) and by name (LIKE) only against the escaped form.
            query.push_str(
                " AND (c.name LIKE ? ESCAPE '\\' OR c.id = ? OR s.target LIKE ? ESCAPE '\\')",
            );
            let escaped = escape_like(conn_name);
            sql_params.push(Box::new(format!("%{}%", escaped)));
            sql_params.push(Box::new(conn_name.to_string()));
            sql_params.push(Box::new(format!("%{}%", escaped)));
        }

        // `checked_sub_signed` instead of `-`: a huge `--days` would push the
        // cutoff out of chrono's range and panic. Such a window covers all
        // history anyway, so it simply means "no cutoff".
        if let Some(cutoff) =
            days.and_then(|d| Utc::now().checked_sub_signed(Duration::days(d.into())))
        {
            query.push_str(" AND s.started_at >= ?");
            sql_params.push(Box::new(cutoff.to_rfc3339()));
        }

        if show_failed_only {
            // Use the structured `status_kind` column so a future
            // `SessionStatus::Error("...")` variant doesn't accidentally
            // match unrelated status names.
            query.push_str(
                " AND (s.status_kind = 'error' OR (s.exit_code IS NOT NULL AND s.exit_code != 0))",
            );
        }

        query.push_str(" ORDER BY s.started_at DESC LIMIT ?");
        sql_params.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&query)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            sql_params.iter().map(|p| p.as_ref()),
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
                session_id: row.get(0)?,
                connection_name: row.get(1)?,
                started_at,
                ended_at,
                status,
                exit_code: row.get(5)?,
                duration,
                recording_path: row.get(6)?,
            });
        }
        Ok(entries)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Session recording FTS5 index & retention
    // ──────────────────────────────────────────────────────────────────────────

    pub fn index_session_text(&self, session_id: Uuid, text: &str) -> Result<()> {
        let id_str = session_id.to_string();
        self.conn.execute(
            "DELETE FROM session_text WHERE session_id = ?",
            params![id_str],
        )?;
        if !text.is_empty() {
            self.conn.execute(
                "INSERT INTO session_text (session_id, content) VALUES (?, ?)",
                params![id_str, text],
            )?;
        }
        Ok(())
    }

    pub fn search_session_text(
        &self,
        query: &str,
        connection_filter: Option<&str>,
        days: Option<u32>,
        limit: usize,
    ) -> Result<Vec<SessionTextHit>> {
        use chrono::{Duration, Utc};

        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Wrap user input as a literal double-quoted phrase so FTS5 syntax
        // (`"`, `*`, `OR`, `NEAR(`, etc.) cannot error or inject operators.
        let fts_phrase = format!("\"{}\"", query.replace('"', "\"\""));

        let mut sql = String::from(
            "SELECT s.id, COALESCE(c.name, s.target, '?'), s.started_at,
                    snippet(session_text, 1, '[', ']', '…', 12)
             FROM session_text
             JOIN sessions s ON s.id = session_text.session_id
             LEFT JOIN connections c ON s.connection_id = c.id
             WHERE session_text MATCH ?",
        );
        let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_phrase)];

        if let Some(conn_name) = connection_filter {
            sql.push_str(
                " AND (c.name LIKE ? ESCAPE '\\' OR c.id = ? OR s.target LIKE ? ESCAPE '\\')",
            );
            let escaped = escape_like(conn_name);
            sql_params.push(Box::new(format!("%{}%", escaped)));
            sql_params.push(Box::new(conn_name.to_string()));
            sql_params.push(Box::new(format!("%{}%", escaped)));
        }

        if let Some(cutoff) =
            days.and_then(|d| Utc::now().checked_sub_signed(Duration::days(d.into())))
        {
            sql.push_str(" AND s.started_at >= ?");
            sql_params.push(Box::new(cutoff.to_rfc3339()));
        }

        sql.push_str(" ORDER BY s.started_at DESC LIMIT ?");
        sql_params.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            sql_params.iter().map(|p| p.as_ref()),
        ))?;

        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            let started_at_str: String = row.get(2)?;
            let started_at =
                chrono::DateTime::parse_from_rfc3339(&started_at_str)?.with_timezone(&Utc);
            hits.push(SessionTextHit {
                session_id: row.get(0)?,
                connection_name: row.get(1)?,
                started_at,
                snippet: row.get(3)?,
            });
        }
        Ok(hits)
    }

    pub fn get_recording_path(&self, session_id_or_prefix: &str) -> Result<Option<(Uuid, String)>> {
        let trimmed = session_id_or_prefix.trim();
        let normalized = if let Ok(uuid) = Uuid::parse_str(trimmed) {
            uuid.to_string()
        } else {
            if trimmed.len() < 6 {
                anyhow::bail!(
                    "session ID prefix '{}' is too short (use at least 6 characters)",
                    trimmed
                );
            }
            trimmed.to_lowercase()
        };

        let like_pattern = format!("{}%", escape_like(&normalized));
        let mut stmt = self.conn.prepare(
            "SELECT id, recording_path
             FROM sessions
             WHERE id LIKE ? ESCAPE '\\'
             ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![like_pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let matches: Vec<(String, Option<String>)> = rows.collect::<rusqlite::Result<_>>()?;

        match matches.len() {
            0 => Ok(None),
            1 => {
                let (id_str, rec_path) = matches.into_iter().next().unwrap();
                match rec_path.filter(|p| !p.is_empty()) {
                    Some(path) => Ok(Some((Uuid::parse_str(&id_str)?, path))),
                    None => Ok(None),
                }
            }
            _ => {
                let ids: Vec<&str> = matches.iter().map(|(id, _)| id.as_str()).collect();
                anyhow::bail!(
                    "ambiguous session ID prefix '{}'; matches: {}",
                    trimmed,
                    ids.join(", ")
                );
            }
        }
    }

    pub fn prune_recordings(&self, older_than_days: u32) -> Result<Vec<String>> {
        use chrono::{Duration, Utc};

        if older_than_days == 0 {
            return Ok(Vec::new());
        }
        let Some(cutoff) = Utc::now().checked_sub_signed(Duration::days(older_than_days.into()))
        else {
            return Ok(Vec::new());
        };
        let cutoff_str = cutoff.to_rfc3339();

        let expired: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, recording_path
                 FROM sessions
                 WHERE recording_path IS NOT NULL AND started_at < ?",
            )?;
            let rows = stmt.query_map(params![cutoff_str], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };

        let mut paths = Vec::with_capacity(expired.len());
        for (id, path) in expired {
            self.conn
                .execute("DELETE FROM session_text WHERE session_id = ?", params![id])?;
            self.conn.execute(
                "UPDATE sessions SET recording_path = NULL WHERE id = ?",
                params![id],
            )?;
            paths.push(path);
        }
        Ok(paths)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Active session management
    // ──────────────────────────────────────────────────────────────────────────

    #[allow(clippy::type_complexity)]
    pub fn get_active_sessions(
        &self,
    ) -> Result<Vec<(String, Option<u32>, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(c.name, s.target, '?'), s.pid, s.started_at
             FROM sessions s
             LEFT JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status_kind = 'active'
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
            "SELECT s.id, COALESCE(c.name, s.target, '?'), s.pid, s.started_at
             FROM sessions s
             LEFT JOIN connections c ON s.connection_id = c.id
             WHERE s.ended_at IS NULL AND s.status_kind = 'active'
               AND (c.name LIKE ?1 ESCAPE '\\' OR c.id = ?2 OR s.target LIKE ?1 ESCAPE '\\')
             ORDER BY s.started_at DESC",
        )?;
        let escaped = escape_like(target);
        let like_pattern = format!("%{}%", escaped);
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

    pub fn mark_session_terminated(&self, session_id: &str, exit_code: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?, status = ?, status_kind = ?, exit_code = ? WHERE id = ?",
            params![
                chrono::Utc::now().to_rfc3339(),
                serde_json::to_string(&crate::models::SessionStatus::Terminated)?,
                SessionStatusKind::Terminated.as_str(),
                exit_code,
                session_id
            ],
        )?;
        Ok(())
    }

    pub fn mark_all_sessions_terminated(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions
             SET ended_at = ?, status = ?, status_kind = ?, exit_code = -1
             WHERE ended_at IS NULL",
            params![
                chrono::Utc::now().to_rfc3339(),
                serde_json::to_string(&crate::models::SessionStatus::Terminated)?,
                SessionStatusKind::Terminated.as_str(),
            ],
        )?;
        Ok(())
    }

    /// Close sessions still recorded as open whose process is gone: the app
    /// or terminal was killed, crashed, or quit before the exit was written.
    /// The exit code is unknown, so it is left NULL. Returns the connection
    /// name and PID of every session closed.
    pub fn close_stale_sessions(&self) -> Result<Vec<(String, Option<u32>)>> {
        let open: Vec<(String, String, Option<u32>)> = {
            let mut stmt = self.conn.prepare(
                "SELECT s.id, COALESCE(c.name, s.target, '?'), s.pid
                 FROM sessions s
                 LEFT JOIN connections c ON s.connection_id = c.id
                 WHERE s.ended_at IS NULL",
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };

        let mut closed = Vec::new();
        for (id, name, pid) in open {
            if pid.and_then(live_pid).is_some() {
                continue;
            }
            self.conn.execute(
                "UPDATE sessions SET ended_at = ?, status = ?, status_kind = ?
                 WHERE id = ? AND ended_at IS NULL",
                params![
                    chrono::Utc::now().to_rfc3339(),
                    serde_json::to_string(&crate::models::SessionStatus::Terminated)?,
                    SessionStatusKind::Terminated.as_str(),
                    id
                ],
            )?;
            closed.push((name, pid));
        }
        Ok(closed)
    }
}

/// Return the PID if it is a plausible session process that is still alive.
///
/// PIDs come from the database, so guard the `u32 → i32` conversion:
/// 0 would signal our own process group, values above `i32::MAX` wrap to
/// negative (signalling a whole process group), and 1 is init.
pub fn live_pid(pid: u32) -> Option<nix::unistd::Pid> {
    let raw = i32::try_from(pid).ok().filter(|&p| p > 1)?;
    let target = nix::unistd::Pid::from_raw(raw);
    // Signal 0 performs only the existence/permission check.
    nix::sys::signal::kill(target, None).ok().map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::live_pid;
    use crate::config::AppConfig;
    use crate::database::Database;
    use crate::models::{Connection, Session};
    use tempfile::tempdir;

    #[test]
    fn live_pid_rejects_dangerous_values() {
        assert!(live_pid(0).is_none());
        assert!(live_pid(1).is_none());
        assert!(live_pid(u32::MAX).is_none());
        assert!(live_pid(i32::MAX as u32 + 1).is_none());
        assert!(live_pid(std::process::id()).is_some());
    }

    #[test]
    fn recording_search_session_text_handles_fts_syntax_and_finds_phrase() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        let conn = Connection::new(
            "web-prod".into(),
            "web.example.com".into(),
            "deploy".into(),
            22,
            None,
            None,
            false,
            None,
        );
        db.add_connection(&conn).unwrap();

        let mut session = Session::new(conn);
        session.recording_path = Some("/tmp/fake.cast".into());
        session.mark_terminated(0);
        db.add_session(&session).unwrap();

        db.index_session_text(
            session.id,
            "deploy@web:~$ sudo systemctl restart nginx\nnginx: configuration file ok\n",
        )
        .unwrap();

        for hostile in [
            "\"",
            "*",
            "OR",
            "NEAR(",
            "\" * OR NEAR( systemctl",
            "NEAR(foo, bar) OR * \"unclosed",
        ] {
            let res = db.search_session_text(hostile, None, None, 10);
            assert!(res.is_ok(), "failed on query {hostile:?}: {res:?}");
        }

        let hits = db
            .search_session_text("systemctl restart nginx", None, None, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, session.id.to_string());
        assert_eq!(hits[0].connection_name, "web-prod");
        assert!(hits[0].snippet.contains("[systemctl]") || hits[0].snippet.contains("systemctl"));
    }
}
