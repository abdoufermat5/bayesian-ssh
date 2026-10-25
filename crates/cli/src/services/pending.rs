//! Guard for route changes a team sync made but the user has not confirmed.
//!
//! A Teams member can edit a shared vault entry. The next sync materializes
//! the new host, port, user, bastion or Kerberos flag into the local
//! connection, and the next connect would hand that host the user's
//! credentials. `Connection::pending_change` remembers the replaced values
//! until the user sees them (`bssh confirm`, or the desktop dialog).

use crate::models::Connection;
use anyhow::{bail, Result};
use serde::Serialize;

/// Marker the desktop app matches to tell this refusal apart from an ssh
/// failure; the rest of the message is the JSON payload.
pub const DESKTOP_PREFIX: &str = "connection-change-pending:";

#[derive(Serialize)]
struct DesktopPendingChange<'a> {
    name: &'a str,
    detected_at: String,
    changes: Vec<String>,
}

/// `field: old → new` lines, one per route field the sync replaced.
pub fn describe(connection: &Connection) -> Vec<String> {
    connection
        .pending_change
        .as_ref()
        .map(|change| change.describe(connection))
        .unwrap_or_default()
}

/// Refuse to use `connection` until its pending change is confirmed.
///
/// Used by every non-interactive path (batch exec, AI runs, the desktop
/// app): the error names the command that clears the pending change.
pub fn ensure_cleared(connection: &Connection) -> Result<()> {
    if connection.pending_change.is_none() {
        return Ok(());
    }
    bail!(refusal(connection))
}

/// The text of that refusal, for callers whose error type is a plain string.
pub fn refusal(connection: &Connection) -> String {
    let details = describe(connection)
        .into_iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "'{}' was changed by a team sync and is not confirmed yet:\n{}\n\
         Run `bssh confirm {}` to accept the new values.",
        connection.name, details, connection.name
    )
}

/// The distinguishable error the desktop app parses to offer its dialog.
pub fn desktop_error(connection: &Connection) -> String {
    let pending = DesktopPendingChange {
        name: &connection.name,
        detected_at: connection
            .pending_change
            .as_ref()
            .map(|c| c.detected_at.to_rfc3339())
            .unwrap_or_default(),
        changes: describe(connection),
    };
    format!(
        "{DESKTOP_PREFIX}{}",
        serde_json::to_string(&pending).unwrap_or_else(|_| "{}".to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn changed() -> Connection {
        let mut conn = Connection::new(
            "db-prod".into(),
            "old.example".into(),
            "alice".into(),
            22,
            None,
            None,
            true,
            None,
        );
        let mut previous = conn.clone();
        previous.host = "db.example".into();
        conn.host = "attacker.example".into();
        conn.pending_change = Some(crate::models::PendingChange::detect(&previous, Utc::now()));
        conn.forward_agent = false;
        conn
    }

    #[test]
    fn unchanged_connection_passes() {
        let conn = Connection::new(
            "db".into(),
            "h".into(),
            "u".into(),
            22,
            None,
            None,
            false,
            None,
        );
        assert!(ensure_cleared(&conn).is_ok());
        assert_eq!(describe(&conn), Vec::<String>::new());
    }

    #[test]
    fn refusal_names_the_command_and_the_change() {
        let conn = changed();
        let err = ensure_cleared(&conn).unwrap_err().to_string();
        assert!(err.contains("bssh confirm db-prod"), "{err}");
        assert!(err.contains("host: db.example → attacker.example"), "{err}");

        let desktop = desktop_error(&conn);
        let json = desktop
            .strip_prefix(DESKTOP_PREFIX)
            .expect("prefix present");
        assert!(json.contains("\"name\":\"db-prod\""), "{json}");
        assert!(json.contains("attacker.example"), "{json}");
    }
}
