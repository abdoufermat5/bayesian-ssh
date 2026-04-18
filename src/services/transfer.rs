//! File transfer service built on the `SftpSession` transport abstraction.
//!
//! Provides `upload` and `download` with optional progress callbacks.
//! Callers supply a `ProgressFn` closure that receives `(bytes_done, total_bytes)`
//! on each chunk; pass `None` to skip progress reporting.

use std::path::Path;

use anyhow::{Context, Result};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use crate::services::transport::{pick_kind, RusshTransport, SubprocessTransport, TransportKind};
use crate::services::transport::types::{SshTransport, TransportError};

const CHUNK_SIZE: usize = 256 * 1024; // 256 KiB
const CHANNEL_CAP: usize = 16;        // outstanding chunks in flight

pub type ProgressFn = Box<dyn Fn(u64, Option<u64>) + Send + 'static>;

pub struct TransferService {
    config: AppConfig,
    #[allow(dead_code)]
    database: Database,
}

impl TransferService {
    pub fn new(config: AppConfig) -> Result<Self> {
        let database = Database::new(&config)?;
        Ok(Self { config, database })
    }

    // ──────────────────────────────────────────────────────────────────────
    // Upload: local → remote
    // ──────────────────────────────────────────────────────────────────────

    /// Upload `local_path` to `remote_path` on `connection`.
    ///
    /// `offset` can be non-zero to resume an interrupted upload.
    /// `mode` is the Unix permission bits for the remote file (e.g. `0o644`).
    pub async fn upload(
        &self,
        connection: &Connection,
        local_path: &Path,
        remote_path: &str,
        offset: u64,
        mode: u32,
        progress: Option<ProgressFn>,
    ) -> Result<u64> {
        let total = fs::metadata(local_path)
            .await
            .with_context(|| format!("stat {}", local_path.display()))?
            .len();

        info!(
            "upload {} → {}:{} ({} bytes, offset {offset})",
            local_path.display(),
            connection.host,
            remote_path,
            total,
        );

        let sftp = self.open_sftp(connection).await?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAP);

        // Spawn a reader that feeds chunks into the channel.
        let local_path_owned = local_path.to_path_buf();
        let reader_handle = tokio::spawn(async move {
            let mut file = fs::File::open(&local_path_owned).await?;
            if offset > 0 {
                use tokio::io::AsyncSeekExt;
                file.seek(std::io::SeekFrom::Start(offset)).await?;
            }
            let mut buf = vec![0u8; CHUNK_SIZE];
            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                if tx.send(buf[..n].to_vec()).await.is_err() {
                    break;
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // Wrap receiver so we can observe progress.
        let (prog_tx, prog_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAP);
        let progress_handle = tokio::spawn(async move {
            let mut incoming = rx;
            let mut bytes_sent: u64 = offset;
            while let Some(chunk) = incoming.recv().await {
                bytes_sent += chunk.len() as u64;
                if let Some(ref cb) = progress {
                    cb(bytes_sent, Some(total));
                }
                debug!("upload progress: {bytes_sent}/{total}");
                if prog_tx.send(chunk).await.is_err() {
                    break;
                }
            }
        });

        let written = sftp
            .write_all(remote_path, offset, prog_rx, mode)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        reader_handle.await?.context("upload reader task")?;
        progress_handle.await.context("upload progress task")?;

        info!("upload complete: {written} bytes written to {remote_path}");
        Ok(written)
    }

    // ──────────────────────────────────────────────────────────────────────
    // Download: remote → local
    // ──────────────────────────────────────────────────────────────────────

    /// Download `remote_path` from `connection` to `local_path`.
    ///
    /// Creates or overwrites `local_path`.  Parent directories must exist.
    pub async fn download(
        &self,
        connection: &Connection,
        remote_path: &str,
        local_path: &Path,
        progress: Option<ProgressFn>,
    ) -> Result<u64> {
        info!(
            "download {}:{} → {}",
            connection.host,
            remote_path,
            local_path.display(),
        );

        let sftp = self.open_sftp(connection).await?;

        // Stat the remote file first so we can report total size.
        let remote_size = sftp.stat(remote_path).await.ok().map(|e| e.size);

        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAP);

        // Drive the SFTP read on a background task.
        let remote_path_owned = remote_path.to_owned();
        let sftp_handle = tokio::spawn(async move {
            sftp.read_all(&remote_path_owned, tx).await
        });

        // Write chunks to the local file, reporting progress.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(local_path)
            .await
            .with_context(|| format!("open {} for writing", local_path.display()))?;

        let mut bytes_received: u64 = 0;
        while let Some(chunk) = rx.recv().await {
            file.write_all(&chunk)
                .await
                .context("write chunk to local file")?;
            bytes_received += chunk.len() as u64;
            if let Some(ref cb) = progress {
                cb(bytes_received, remote_size);
            }
            debug!("download progress: {bytes_received}");
        }

        file.flush().await.context("flush local file")?;

        let read = sftp_handle
            .await
            .context("SFTP read task panic")?
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        info!("download complete: {read} bytes saved to {}", local_path.display());
        Ok(read)
    }

    // ──────────────────────────────────────────────────────────────────────
    // Internal: resolve transport and open SFTP session
    // ──────────────────────────────────────────────────────────────────────

    async fn open_sftp(
        &self,
        connection: &Connection,
    ) -> Result<Box<dyn crate::services::transport::types::SftpSession>> {
        let kind = pick_kind(connection, &self.config);
        let result: Result<_, TransportError> = match kind {
            TransportKind::Native => {
                let t = RusshTransport::new(self.config.clone());
                match t.open_sftp(connection).await {
                    Err(TransportError::Fallback(e)) => {
                        tracing::warn!("native SFTP fallback ({e}), subprocess has no SFTP");
                        return Err(anyhow::anyhow!(
                            "SFTP requires native transport; force_subprocess is set"
                        ));
                    }
                    other => other,
                }
            }
            TransportKind::Subprocess => {
                // SubprocessTransport always returns Fallback for SFTP.
                let t = SubprocessTransport::new(self.config.clone());
                match t.open_sftp(connection).await {
                    Err(e) => return Err(anyhow::anyhow!("SFTP unavailable via subprocess: {e}")),
                    Ok(s) => Ok(s),
                }
            }
        };
        result.map_err(|e| anyhow::anyhow!("{e}"))
    }
}
