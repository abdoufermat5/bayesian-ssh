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
use tracing::{debug, info, warn};

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use crate::services::transport::scp_impl;
use crate::services::transport::types::{SshTransport, TransportError};
use crate::services::transport::{pick_kind, RusshTransport, SubprocessTransport, TransportKind};

const CHUNK_SIZE: usize = 256 * 1024; // 256 KiB
const CHANNEL_CAP: usize = 16; // outstanding chunks in flight

pub type ProgressFn = Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>;

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
        let sftp_handle = tokio::spawn(async move { sftp.read_all(&remote_path_owned, tx).await });

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

        info!(
            "download complete: {read} bytes saved to {}",
            local_path.display()
        );
        Ok(read)
    }

    // ──────────────────────────────────────────────────────────────────────
    // Recursive upload: local directory → remote directory
    // ──────────────────────────────────────────────────────────────────────

    /// Recursively upload a local directory to a remote path.
    ///
    /// Returns `(files_transferred, total_bytes)`.
    pub async fn upload_recursive(
        &self,
        connection: &Connection,
        local_dir: &Path,
        remote_dir: &str,
        mode: u32,
        progress: Option<&ProgressFn>,
    ) -> Result<(u64, u64)> {
        let sftp = self.open_sftp(connection).await?;
        let mut file_count = 0u64;
        let mut total_bytes = 0u64;
        Self::upload_dir_inner(
            &*sftp,
            local_dir,
            remote_dir,
            mode,
            progress,
            &mut file_count,
            &mut total_bytes,
        )
        .await?;
        Ok((file_count, total_bytes))
    }

    fn upload_dir_inner<'a>(
        sftp: &'a dyn crate::services::transport::types::SftpSession,
        local_dir: &'a Path,
        remote_dir: &'a str,
        mode: u32,
        progress: Option<&'a ProgressFn>,
        file_count: &'a mut u64,
        total_bytes: &'a mut u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            // Create the remote directory (ignore "already exists" errors)
            match sftp.mkdir(remote_dir, 0o755).await {
                Ok(()) => debug!("created remote dir {remote_dir}"),
                Err(_) => debug!("remote dir {remote_dir} may already exist, continuing"),
            }

            let mut entries = fs::read_dir(local_dir)
                .await
                .with_context(|| format!("read local dir {}", local_dir.display()))?;

            while let Some(entry) = entries.next_entry().await? {
                let file_type = entry.file_type().await?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                let local_child = entry.path();
                let remote_child = format!("{}/{}", remote_dir.trim_end_matches('/'), name_str);

                if file_type.is_dir() {
                    Self::upload_dir_inner(
                        sftp,
                        &local_child,
                        &remote_child,
                        mode,
                        progress,
                        file_count,
                        total_bytes,
                    )
                    .await?;
                } else if file_type.is_file() {
                    info!("uploading {} → {remote_child}", local_child.display());

                    let meta = fs::metadata(&local_child).await?;
                    let file_size = meta.len();

                    let (tx, rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAP);
                    let local_owned = local_child.clone();
                    let reader = tokio::spawn(async move {
                        let mut file = fs::File::open(&local_owned).await?;
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

                    let written = sftp
                        .write_all(&remote_child, 0, rx, mode)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;

                    reader.await?.context("upload reader task")?;

                    *file_count += 1;
                    *total_bytes += written;

                    if let Some(ref cb) = progress {
                        cb(*total_bytes, Some(file_size));
                    }
                } else if file_type.is_symlink() {
                    warn!("skipping symlink: {}", local_child.display());
                }
            }
            Ok(())
        })
    }

    // ──────────────────────────────────────────────────────────────────────
    // Recursive download: remote directory → local directory
    // ──────────────────────────────────────────────────────────────────────

    /// Recursively download a remote directory to a local path.
    ///
    /// Returns `(files_transferred, total_bytes)`.
    pub async fn download_recursive(
        &self,
        connection: &Connection,
        remote_dir: &str,
        local_dir: &Path,
        progress: Option<&ProgressFn>,
    ) -> Result<(u64, u64)> {
        let sftp = self.open_sftp(connection).await?;
        let mut file_count = 0u64;
        let mut total_bytes = 0u64;
        Self::download_dir_inner(
            &*sftp,
            remote_dir,
            local_dir,
            progress,
            &mut file_count,
            &mut total_bytes,
        )
        .await?;
        Ok((file_count, total_bytes))
    }

    fn download_dir_inner<'a>(
        sftp: &'a dyn crate::services::transport::types::SftpSession,
        remote_dir: &'a str,
        local_dir: &'a Path,
        progress: Option<&'a ProgressFn>,
        file_count: &'a mut u64,
        total_bytes: &'a mut u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            // Create local directory
            fs::create_dir_all(local_dir)
                .await
                .with_context(|| format!("create local dir {}", local_dir.display()))?;

            let entries = sftp
                .list(remote_dir)
                .await
                .map_err(|e| anyhow::anyhow!("list {remote_dir}: {e}"))?;

            for entry in entries {
                // Skip . and ..
                if entry.name == "." || entry.name == ".." {
                    continue;
                }
                let remote_child = format!("{}/{}", remote_dir.trim_end_matches('/'), entry.name);
                let local_child = local_dir.join(&entry.name);

                if entry.is_dir {
                    Self::download_dir_inner(
                        sftp,
                        &remote_child,
                        &local_child,
                        progress,
                        file_count,
                        total_bytes,
                    )
                    .await?;
                } else if entry.is_symlink {
                    warn!("skipping symlink: {remote_child}");
                } else {
                    info!("downloading {remote_child} → {}", local_child.display());

                    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAP);
                    let remote_owned = remote_child.clone();
                    let sftp_read = sftp.read_all(&remote_owned, tx);

                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&local_child)
                        .await
                        .with_context(|| format!("open {} for writing", local_child.display()))?;

                    let write_task = async {
                        let mut received = 0u64;
                        while let Some(chunk) = rx.recv().await {
                            file.write_all(&chunk).await?;
                            received += chunk.len() as u64;
                        }
                        file.flush().await?;
                        Ok::<u64, anyhow::Error>(received)
                    };

                    let (read_result, write_result) = tokio::join!(sftp_read, write_task);
                    read_result.map_err(|e| anyhow::anyhow!("{e}"))?;
                    let received = write_result?;

                    *file_count += 1;
                    *total_bytes += received;

                    if let Some(ref cb) = progress {
                        cb(*total_bytes, None);
                    }
                }
            }
            Ok(())
        })
    }

    // ──────────────────────────────────────────────────────────────────────
    // SCP fallback: used when SFTP is unavailable (e.g. interactive bastion)
    // ──────────────────────────────────────────────────────────────────────

    /// Upload via SCP (bastion-aware). Falls back to this when SFTP is not
    /// available on the subprocess transport.
    pub async fn scp_upload(
        &self,
        connection: &Connection,
        local_path: &Path,
        remote_path: &str,
        recursive: bool,
    ) -> Result<()> {
        info!(
            "scp upload {} → {}:{} (recursive={recursive})",
            local_path.display(),
            connection.host,
            remote_path,
        );
        scp_impl::scp_upload(connection, local_path, remote_path, recursive).await
    }

    /// Download via SCP (bastion-aware).
    pub async fn scp_download(
        &self,
        connection: &Connection,
        remote_path: &str,
        local_path: &Path,
        recursive: bool,
    ) -> Result<()> {
        info!(
            "scp download {}:{} → {} (recursive={recursive})",
            connection.host,
            remote_path,
            local_path.display(),
        );
        scp_impl::scp_download(connection, remote_path, local_path, recursive).await
    }

    /// Returns `true` when SFTP is available for this connection, `false`
    /// when the caller should use SCP instead.
    pub fn has_sftp(&self, connection: &Connection) -> bool {
        let kind = pick_kind(connection, &self.config);
        !matches!(kind, TransportKind::Subprocess)
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
