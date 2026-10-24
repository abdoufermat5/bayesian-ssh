//! File access for `read_file` and `edit_file` (SFTP), behind a trait so tests use a fake.
//!
//! The SFTP session uses the connection's own auth (same as `bssh sftp`);
//! it is not jailed. Policy and deny paths are enforced by the caller before it gets here.

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transport::{RusshTransport, SshTransport};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct FileRead {
    /// Bytes from `offset`, at most `max_bytes`.
    pub data: Vec<u8>,
    /// Size of the whole file on the host.
    pub size: u64,
    /// Unix permission bits when known.
    pub mode: Option<u32>,
}

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait FileAccess: Send + Sync {
    async fn read(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead>;

    /// Replace the file's content (create if missing).
    async fn write(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()>;
}

/// SFTP over russh.
pub struct SftpFiles {
    transport: RusshTransport,
}

impl SftpFiles {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            transport: RusshTransport::new(config.clone()),
        }
    }
}

#[async_trait]
impl FileAccess for SftpFiles {
    async fn read(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead> {
        let sftp = self
            .transport
            .open_sftp(conn)
            .await
            .map_err(|e| anyhow!("sftp: {e}"))?;
        let st = sftp
            .stat(path)
            .await
            .map_err(|e| anyhow!("stat {path}: {e}"))?;
        if st.is_dir {
            return Err(anyhow!("{path} is a directory"));
        }
        let want = offset.saturating_add(max_bytes as u64);
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        let reader = sftp.read_all(path, tx);
        let collector = async move {
            let mut buf: Vec<u8> = Vec::new();
            while let Some(chunk) = rx.recv().await {
                buf.extend_from_slice(&chunk);
                if buf.len() as u64 >= want {
                    break;
                }
            }
            buf
        };
        let (res, buf) = tokio::join!(reader, collector);
        // The reader fails when we stop listening after the range is full: that is fine.
        if let Err(e) = res {
            if (buf.len() as u64) < want && (buf.len() as u64) < st.size {
                return Err(anyhow!("read {path}: {e}"));
            }
        }
        let start = (offset as usize).min(buf.len());
        let end = (start + max_bytes).min(buf.len());
        Ok(FileRead {
            data: buf[start..end].to_vec(),
            size: st.size,
            mode: Some(st.mode & 0o7777),
        })
    }

    async fn write(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        let sftp = self
            .transport
            .open_sftp(conn)
            .await
            .map_err(|e| anyhow!("sftp: {e}"))?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
        let owned = data.to_vec();
        let feeder = async move {
            for chunk in owned.chunks(32 * 1024) {
                if tx.send(chunk.to_vec()).await.is_err() {
                    break;
                }
            }
        };
        let writer = sftp.write_all(path, 0, rx, mode.unwrap_or(0o644), None);
        let (res, ()) = tokio::join!(writer, feeder);
        res.map_err(|e| anyhow!("write {path}: {e}"))?;
        Ok(())
    }
}
