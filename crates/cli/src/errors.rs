use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Connection not found: {0}")]
    ConnectionNotFound(String),

    #[error("Duplicate connection: {0}")]
    DuplicateConnection(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub fn report_cli_error(error: &anyhow::Error) {
    eprintln!("Error: {error}");

    if let Some(suggestion) = suggestion_for(error) {
        eprintln!("Suggestion: {suggestion}");
    }
}

fn suggestion_for(error: &anyhow::Error) -> Option<&'static str> {
    if error_contains(error, "database file does not exist") {
        return Some("run `bssh doctor` to initialize and inspect the active environment");
    }

    if error_contains(error, "backup file does not exist") {
        return Some("check the backup path or run `bssh backup` to create a new backup first");
    }

    if error_contains(error, "connection") && error_contains(error, "not found") {
        return Some(
            "run `bssh list` to see saved connections or `bssh add <name> <host>` to create one",
        );
    }

    if error_contains(error, "permission denied") {
        return Some("check file permissions and ownership for the path shown above");
    }

    None
}

fn error_contains(error: &anyhow::Error, needle: &str) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().to_lowercase().contains(needle))
}
