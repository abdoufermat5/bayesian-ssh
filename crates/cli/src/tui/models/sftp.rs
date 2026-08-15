use crate::services::transport::types::RemoteEntry;

/// Messages sent back from async SFTP tasks to the event loop.
pub enum SftpMsg {
    /// A directory listing completed successfully.
    Listed {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    /// A file was downloaded to the given local path.
    Downloaded {
        remote: String,
        local: String,
        bytes: u64,
    },
    /// A local file was uploaded to the remote.
    Uploaded {
        local: String,
        remote: String,
        bytes: u64,
    },
    /// A remote file/dir was deleted.
    Removed { path: String },
    /// A remote directory was created.
    DirCreated { path: String },
    /// A remote entry was renamed.
    Renamed { from: String, to: String },
    /// Any SFTP error.
    Error(String),
}
