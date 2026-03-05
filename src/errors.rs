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
