//! SFTP session backed by `russh-sftp`.
//!
//! `RusshSftpSession` wraps a `russh_sftp::client::SftpSession` and implements
//! the `SftpSession` trait so callers stay transport-agnostic.

use anyhow::anyhow;
use async_trait::async_trait;
use russh_sftp::client::SftpSession as RawSftp;
use russh_sftp::protocol::OpenFlags;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::debug;

use super::types::{RemoteEntry, SftpSession, TransportError};

// ──────────────────────────────────────────────────────────────────────────────
// Helper — convert russh_sftp errors to TransportError
// ──────────────────────────────────────────────────────────────────────────────

fn sftp_err(e: russh_sftp::client::error::Error) -> TransportError {
    TransportError::Permanent(anyhow!("SFTP error: {e}"))
}

// ──────────────────────────────────────────────────────────────────────────────
// RusshSftpSession
// ──────────────────────────────────────────────────────────────────────────────

/// `Arc`-wrapped so the session can be kept alive while individual file handles
/// are open (which hold a clone).
pub struct RusshSftpSession {
    inner: Arc<RawSftp>,
}

impl RusshSftpSession {
    pub fn new(inner: RawSftp) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// SftpSession impl
// ──────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl SftpSession for RusshSftpSession {
    async fn list(&self, path: &str) -> Result<Vec<RemoteEntry>, TransportError> {
        let read_dir = self.inner.read_dir(path).await.map_err(sftp_err)?;
        let entries = read_dir
            .into_iter()
            .map(|entry| {
                let meta = entry.metadata();
                let ft = meta.file_type();
                RemoteEntry {
                    name: entry.file_name(),
                    path: PathBuf::from(path).join(entry.file_name()),
                    is_dir: ft.is_dir(),
                    is_symlink: ft.is_symlink(),
                    size: meta.size.unwrap_or(0),
                    mode: meta.permissions.unwrap_or(0),
                    modified_unix: meta.mtime.map(|t| t as i64),
                }
            })
            .collect();
        Ok(entries)
    }

    async fn stat(&self, path: &str) -> Result<RemoteEntry, TransportError> {
        let meta = self.inner.metadata(path).await.map_err(sftp_err)?;
        let ft = meta.file_type();
        let name = PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned());
        Ok(RemoteEntry {
            name,
            path: PathBuf::from(path),
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
            size: meta.size.unwrap_or(0),
            mode: meta.permissions.unwrap_or(0),
            modified_unix: meta.mtime.map(|t| t as i64),
        })
    }

    async fn mkdir(&self, path: &str, _mode: u32) -> Result<(), TransportError> {
        // russh_sftp's create_dir does not accept a mode parameter; the server
        // applies its own umask. Pass the desired mode via set_metadata after.
        self.inner.create_dir(path).await.map_err(sftp_err)?;
        Ok(())
    }

    async fn remove(&self, path: &str) -> Result<(), TransportError> {
        // Try file removal first; fall back to directory removal.
        match self.inner.remove_file(path).await {
            Ok(()) => Ok(()),
            Err(_) => self.inner.remove_dir(path).await.map_err(sftp_err),
        }
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), TransportError> {
        self.inner.rename(from, to).await.map_err(sftp_err)
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<(), TransportError> {
        let attrs = russh_sftp::protocol::FileAttributes {
            permissions: Some(mode),
            ..Default::default()
        };
        self.inner.set_metadata(path, attrs).await.map_err(sftp_err)
    }

    async fn read_all(
        &self,
        path: &str,
        sink: mpsc::Sender<Vec<u8>>,
    ) -> Result<u64, TransportError> {
        let mut file = self.inner.open(path).await.map_err(sftp_err)?;
        let mut buf = vec![0u8; 32 * 1024];
        let mut total: u64 = 0;

        loop {
            let n = file
                .read(&mut buf)
                .await
                .map_err(|e| TransportError::Permanent(anyhow!("SFTP read: {e}")))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            debug!("sftp read_all: {n} bytes from {path} (total {total})");
            sink.send(buf[..n].to_vec()).await.map_err(|_| {
                TransportError::Permanent(anyhow!("SFTP read: sink channel closed"))
            })?;
        }

        file.shutdown()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("SFTP close: {e}")))?;
        Ok(total)
    }

    async fn write_all(
        &self,
        path: &str,
        offset: u64,
        mut source: mpsc::Receiver<Vec<u8>>,
        mode: u32,
    ) -> Result<u64, TransportError> {
        // Open or create the file; seek to `offset` for resume support.
        let attrs = russh_sftp::protocol::FileAttributes {
            permissions: Some(mode),
            ..Default::default()
        };

        let flags = if offset == 0 {
            // Fresh write: create + truncate
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE
        } else {
            // Resume: write + create (no truncate)
            OpenFlags::WRITE | OpenFlags::CREATE
        };

        let mut file = self
            .inner
            .open_with_flags_and_attributes(path, flags, attrs)
            .await
            .map_err(sftp_err)?;

        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| TransportError::Permanent(anyhow!("SFTP seek: {e}")))?;
        }

        let mut total: u64 = 0;
        while let Some(chunk) = source.recv().await {
            file.write_all(&chunk)
                .await
                .map_err(|e| TransportError::Permanent(anyhow!("SFTP write: {e}")))?;
            total += chunk.len() as u64;
            debug!(
                "sftp write_all: {} bytes to {path} (total {total})",
                chunk.len()
            );
        }

        file.sync_all()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("SFTP sync: {e}")))?;
        file.shutdown()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("SFTP close: {e}")))?;
        Ok(total)
    }
}
