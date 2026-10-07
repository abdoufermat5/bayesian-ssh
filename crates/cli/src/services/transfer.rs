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
use crate::services::transport::{pick_kind, TransportKind};

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

        // Spawn a reader that feeds chunks into the channel. We keep
        // ownership of a small in-flight budget (CHANNEL_CAP) so a
        // slow downstream writer does NOT cause the reader to block
        // indefinitely on a full bounded channel. If `tx.send`
        // fails, the writer has hung up and we stop reading.
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
                    // The writer hung up (error or cancellation); stop
                    // reading so the task can complete.
                    break;
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // Stream chunks straight from the reader to the SFTP writer.
        // We report progress inline via the `on_chunk` callback (called
        // once per chunk written by the SFTP session). The previous
        // design used a second mpsc channel for progress, which added a
        // back-pressure hop and could hang when the writer was slow.
        let bytes_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(offset));
        let on_chunk: Option<Box<dyn Fn(usize) + Send + Sync>> = if let Some(cb) = progress {
            let counter = bytes_counter.clone();
            Some(Box::new(move |n| {
                let total_now =
                    counter.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed) + n as u64;
                cb(total_now, Some(total));
            }))
        } else {
            None
        };

        let written = sftp
            .write_all(remote_path, offset, rx, mode, on_chunk)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // A local read error drops the sender, which the writer cannot tell
        // apart from EOF — so a failed reader means the remote file is
        // truncated and must be reported. (Early exit because the writer hung
        // up returns `Ok`.)
        reader_handle
            .await
            .context("upload reader task")?
            .with_context(|| format!("read {}", local_path.display()))?;

        info!("upload complete: {written} bytes written to {remote_path}");
        Ok(written)
    }

    // ──────────────────────────────────────────────────────────────────────
    // Download: remote → local
    // ──────────────────────────────────────────────────────────────────────

    /// Download `remote_path` from `connection` to `local_path`.
    ///
    /// Writes to a sibling temp file and atomically renames into place on
    /// success, so a partial download never clobbers the destination. The
    /// temp file is created exclusively (`O_EXCL`) so a pre-existing
    /// symlink cannot redirect the write.
    pub async fn download(
        &self,
        connection: &Connection,
        remote_path: &str,
        local_path: &Path,
        progress: Option<ProgressFn>,
    ) -> Result<u64> {
        // Reject `..` path components in the remote name (defense in
        // depth — SFTP is a separate trust boundary but the local
        // caller can still be surprised).
        reject_traversal(remote_path).map_err(|e| anyhow::anyhow!("invalid remote path: {e}"))?;

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

        // Drive the SFTP read on a background task. If the channel
        // closes (because we error out below), the reader is signalled
        // and aborts — no silent hang on a slow writer.
        let remote_path_owned = remote_path.to_owned();
        let sftp_handle = tokio::spawn(async move { sftp.read_all(&remote_path_owned, tx).await });

        // Atomic write: create a temp file alongside the destination,
        // rename on success.
        let temp_path = partial_path(local_path);

        let mut file = open_no_follow_create(&temp_path).await?;
        let mut bytes_received: u64 = 0;
        while let Some(chunk) = rx.recv().await {
            if let Err(e) = file.write_all(&chunk).await {
                // Best-effort cleanup so we don't leave a half-written
                // file lying around.
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(anyhow::Error::from(e).context("write chunk to local file"));
            }
            bytes_received += chunk.len() as u64;
            if let Some(ref cb) = progress {
                cb(bytes_received, remote_size);
            }
            debug!("download progress: {bytes_received}");
        }

        if let Err(e) = file.flush().await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(anyhow::Error::from(e).context("flush local file"));
        }
        drop(file);

        let read = match sftp_handle.await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(anyhow::anyhow!("SFTP read error: {e}"));
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(anyhow::Error::from(e).context("SFTP read task panic"));
            }
        };

        // Atomic rename into place.
        if let Err(e) = tokio::fs::rename(&temp_path, local_path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(anyhow::Error::from(e).context("rename temp file into place"));
        }

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
                // Use the entry's metadata (no second stat call) to avoid
                // a TOCTOU race where the file is replaced between
                // `read_dir` and `fs::metadata`.
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

                    let file_size = entry.metadata().await?.len();

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
                        .write_all(&remote_child, 0, rx, mode, None)
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
                // Names come from the (untrusted) server. A name containing a
                // separator, or an absolute one (`Path::join` replaces the
                // base), would let a malicious server write anywhere locally.
                if !is_plain_file_name(&entry.name) {
                    warn!("skipping remote entry with unsafe name: {:?}", entry.name);
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

                    // Atomic write to a sibling temp file, then rename
                    // on success. Exclusive create so a pre-existing
                    // symlink can't be followed.
                    let temp_path = partial_path(&local_child);
                    let mut file = match open_no_follow_create(&temp_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            return Err(
                                e.context(format!("open {} for writing", local_child.display()))
                            );
                        }
                    };

                    let write_task = async {
                        let mut received = 0u64;
                        while let Some(chunk) = rx.recv().await {
                            file.write_all(&chunk).await?;
                            received += chunk.len() as u64;
                        }
                        file.flush().await?;
                        drop(file);
                        Ok::<u64, anyhow::Error>(received)
                    };

                    // Only move the file into place once BOTH sides succeeded:
                    // a failed SFTP read just closes the channel, which the
                    // writer sees as a normal EOF.
                    let (read_result, write_result) = tokio::join!(sftp_read, write_task);
                    let outcome = match (read_result, write_result) {
                        (Err(e), _) => Err(anyhow::anyhow!("{e}")),
                        (Ok(_), Err(e)) => Err(e),
                        (Ok(_), Ok(n)) => tokio::fs::rename(&temp_path, &local_child)
                            .await
                            .map(|()| n)
                            .context("rename temp file into place"),
                    };
                    let received = match outcome {
                        Ok(n) => n,
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&temp_path).await;
                            return Err(e);
                        }
                    };

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
        let conn = connection.clone();
        crate::services::transport::execute_with_fallback(connection, &self.config, |transport| {
            let conn_clone = conn.clone();
            Box::pin(async move { transport.open_sftp(&conn_clone).await })
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Path-traversal guard + symlink-safe local open
// ─────────────────────────────────────────────────────────────────────────────

/// Reject paths whose segments include `..`. We don't try to canonicalize
/// the path (which would defeat relative paths) — we only forbid the
/// obvious escape sequences.
fn reject_traversal(p: &str) -> std::result::Result<(), &'static str> {
    for segment in p.split(['/', '\\']) {
        if segment == ".." {
            return Err("path contains '..' segment");
        }
    }
    Ok(())
}

/// True when `name` is a single, plain path component (no separators, not
/// absolute, not `.`/`..`, no NUL) and is therefore safe to `join` onto a
/// local directory.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Sibling temp path for an in-progress download: `<dest>.<pid>.<n>.bssh-partial`.
/// The per-process counter keeps concurrent downloads to the same target
/// from sharing (and corrupting) one temp file.
fn partial_path(dest: &Path) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut s = dest.as_os_str().to_owned();
    s.push(format!(".{}.{n}.bssh-partial", std::process::id()));
    s.into()
}

/// Create a fresh local file for writing without ever following a
/// symlink. A stale entry left by a crashed run is unlinked first (unlink
/// removes a symlink itself, not its target), then the file is created with
/// `O_CREAT|O_EXCL`, which fails rather than follow anything planted in
/// between. This avoids hard-coding the per-architecture `O_NOFOLLOW` value.
async fn open_no_follow_create(path: &Path) -> Result<tokio::fs::File> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(anyhow::Error::from(e).context(format!("remove stale {}", path.display())))
        }
    }
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .with_context(|| format!("create {} for writing", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{is_plain_file_name, partial_path, reject_traversal};
    use std::path::Path;

    #[test]
    fn remote_names_must_be_single_components() {
        assert!(is_plain_file_name("report.txt"));
        assert!(is_plain_file_name("..hidden"));
        assert!(!is_plain_file_name(""));
        assert!(!is_plain_file_name("."));
        assert!(!is_plain_file_name(".."));
        assert!(!is_plain_file_name("../../.bashrc"));
        assert!(!is_plain_file_name("/etc/cron.d/x"));
        assert!(!is_plain_file_name("a\\b"));
    }

    #[test]
    fn partial_paths_are_unique_siblings() {
        let a = partial_path(Path::new("/dl/file.bin"));
        let b = partial_path(Path::new("/dl/file.bin"));
        assert_ne!(a, b);
        assert_eq!(a.parent(), Some(Path::new("/dl")));
        assert!(a.to_string_lossy().ends_with(".bssh-partial"));
    }

    #[test]
    fn rejects_traversal_segments() {
        assert!(reject_traversal("../etc/passwd").is_err());
        assert!(reject_traversal("/foo/../bar").is_err());
        assert!(reject_traversal("foo\\..\\bar").is_err());
        assert!(reject_traversal("/absolute/path").is_ok());
        assert!(reject_traversal("relative/path").is_ok());
        assert!(reject_traversal("..hidden").is_ok()); // not a traversal
        assert!(reject_traversal("a..b").is_ok()); // not a segment
    }
}
