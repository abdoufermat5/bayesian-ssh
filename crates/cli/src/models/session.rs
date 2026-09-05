use crate::models::Connection;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub connection: Connection,
    #[serde(default)]
    pub transport: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
}

/// Stable, lower-case discriminator stored in the `sessions.status_kind`
/// column. Used for indexed queries (e.g. "active sessions", "failed
/// sessions") without parsing the JSON `status` blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatusKind {
    Starting,
    Active,
    Disconnected,
    Terminated,
    Error,
}

impl SessionStatusKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatusKind::Starting => "starting",
            SessionStatusKind::Active => "active",
            SessionStatusKind::Disconnected => "disconnected",
            SessionStatusKind::Terminated => "terminated",
            SessionStatusKind::Error => "error",
        }
    }
}

impl std::str::FromStr for SessionStatusKind {
    type Err = std::convert::Infallible;
    /// Parse a `status_kind` string back into the enum. Unknown values
    /// map to `SessionStatusKind::Error` for forward-compatibility.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "starting" => Ok(SessionStatusKind::Starting),
            "active" => Ok(SessionStatusKind::Active),
            "disconnected" => Ok(SessionStatusKind::Disconnected),
            "terminated" => Ok(SessionStatusKind::Terminated),
            _ => Ok(SessionStatusKind::Error),
        }
    }
}

impl SessionStatus {
    pub fn kind(&self) -> SessionStatusKind {
        match self {
            SessionStatus::Starting => SessionStatusKind::Starting,
            SessionStatus::Active => SessionStatusKind::Active,
            SessionStatus::Disconnected => SessionStatusKind::Disconnected,
            SessionStatus::Terminated => SessionStatusKind::Terminated,
            SessionStatus::Error(_) => SessionStatusKind::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Starting,
    Active,
    Disconnected,
    Terminated,
    Error(String),
}

impl Session {
    pub fn new(connection: Connection) -> Self {
        Self {
            id: Uuid::new_v4(),
            connection,
            transport: None,
            started_at: Utc::now(),
            ended_at: None,
            status: SessionStatus::Starting,
            pid: None,
            exit_code: None,
        }
    }

    pub fn mark_active(&mut self, pid: u32) {
        self.status = SessionStatus::Active;
        self.pid = Some(pid);
    }

    #[allow(dead_code)]
    pub fn mark_disconnected(&mut self) {
        self.status = SessionStatus::Disconnected;
        self.ended_at = Some(Utc::now());
    }

    pub fn mark_terminated(&mut self, exit_code: i32) {
        self.status = SessionStatus::Terminated;
        self.ended_at = Some(Utc::now());
        self.exit_code = Some(exit_code);
    }

    pub fn mark_error(&mut self, error: String) {
        self.status = SessionStatus::Error(error);
        self.ended_at = Some(Utc::now());
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        matches!(self.status, SessionStatus::Active)
    }

    #[allow(dead_code)]
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.ended_at
            .or_else(|| Some(Utc::now()))
            .map(|end| end - self.started_at)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub average_duration: Option<chrono::Duration>,
    pub success_rate: f64,
}

/// Session history entry for display purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryEntry {
    pub connection_name: String,
    pub started_at: DateTime<Utc>,
    #[allow(dead_code)]
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
    pub exit_code: Option<i32>,
    pub duration: Option<chrono::Duration>,
}
