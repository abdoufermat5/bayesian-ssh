//! Error type returned by `#[tauri::command]` functions.
//!
//! The frontend receives the error as a plain JSON string, so the type
//! serializes as its `Display` text.

use serde::{Serialize, Serializer};

/// Error returned to the frontend by Tauri commands.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// A user-facing message that is already fully formed.
    #[error("{0}")]
    Message(String),
    /// An error bubbled up from the core crate or another `anyhow` source.
    #[error("{0}")]
    Other(#[from] anyhow::Error),
    /// An I/O failure.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A JSON (de)serialization failure.
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    /// A window-system failure reported by Tauri.
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    /// A malformed UUID argument.
    #[error("{0}")]
    Uuid(#[from] uuid::Error),
    /// A connection field the core refused.
    #[error("{0}")]
    Validation(#[from] bayesian_ssh::models::ValidationError),
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_owned())
    }
}

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of(e: &CommandError) -> String {
        serde_json::to_string(e).unwrap()
    }

    #[test]
    fn serializes_like_the_old_string_error() {
        let old_json = serde_json::to_string("boom \"quoted\"").unwrap();
        assert_eq!(json_of(&CommandError::from("boom \"quoted\"")), old_json);
        assert_eq!(
            json_of(&CommandError::from("boom \"quoted\"".to_string())),
            old_json
        );
    }

    #[test]
    fn anyhow_and_io_serialize_as_their_display_text() {
        let e = CommandError::from(anyhow::anyhow!("db failed"));
        assert_eq!(json_of(&e), "\"db failed\"");
        let e = CommandError::from(std::io::Error::other("disk full"));
        assert_eq!(json_of(&e), "\"disk full\"");
    }
}
