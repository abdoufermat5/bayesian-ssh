//! Multi-host upload service.
//!
//! Uploads one local file or directory to many connections concurrently,
//! reporting progress as serializable [`UploadEvent`]s so both the CLI
//! (progress bars) and the desktop GUI (Tauri events) can consume the same
//! stream. Per-host failures never abort the run: they are returned as
//! [`HostUploadResult`]s in the same order as the requested targets.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transfer::{ProgressFn, TransferService};
use crate::services::transport::types::SftpSession;

/// One upload request, shared across every target host.
#[derive(Clone, Debug)]
pub struct UploadRequest {
    /// Local file or directory to upload.
    pub local: PathBuf,
    /// Destination on the remote host. A trailing `/` (or an existing remote
    /// directory) means "upload into it"; a local file then keeps its basename.
    pub remote: String,
    /// Remote file permissions (e.g. `0o644`).
    pub mode: u32,
    /// Resume offset; only valid for a single file on a single host.
    pub offset: u64,
    /// Required to upload a directory tree.
    pub recursive: bool,
}

/// Streaming progress event emitted per host.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UploadEvent {
    Started {
        connection_id: Uuid,
        name: String,
        host: String,
    },
    Progress {
        connection_id: Uuid,
        bytes_done: u64,
        bytes_total: Option<u64>,
    },
    Finished {
        result: HostUploadResult,
    },
}

/// Outcome of the upload to one host.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostUploadResult {
    pub connection_id: Uuid,
    pub name: String,
    pub host: String,
    pub ok: bool,
    pub error: Option<String>,
    pub files: u64,
    pub bytes: u64,
    pub via_scp: bool,
    /// Final remote path the upload resolved to.
    pub remote_path: String,
    pub elapsed_ms: u64,
}

/// Callback invoked for every [`UploadEvent`].
pub type UploadEventFn = Arc<dyn Fn(UploadEvent) + Send + Sync>;

/// A boxed, `Send` future with a borrowed lifetime.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Upload `req.local` to every connection in `targets`, concurrently.
///
/// Up-front validation problems (missing local path, a directory without
/// `recursive`, misuse of `offset`, no targets) return `Err` before any host
/// is touched. Per-host failures are reported in the returned vector — never
/// as an error — in the same order as `targets`.
///
/// The returned future is cancel-safe: dropping it aborts the in-flight host
/// tasks (used by the GUI to cancel an upload).
pub async fn upload_to_hosts(
    config: &AppConfig,
    targets: Vec<Connection>,
    req: UploadRequest,
    parallel: usize,
    on_event: Option<UploadEventFn>,
) -> Result<Vec<HostUploadResult>> {
    if targets.is_empty() {
        bail!("no target hosts to upload to");
    }
    if req.offset > 0 && (targets.len() > 1 || req.recursive) {
        bail!("--offset can only be used for a single file on a single host");
    }

    let meta = tokio::fs::metadata(&req.local)
        .await
        .with_context(|| format!("stat {}", req.local.display()))?;
    let local_is_dir = meta.is_dir();
    if local_is_dir && !req.recursive {
        bail!(
            "'{}' is a directory — pass --recursive (-r) to upload directories",
            req.local.display()
        );
    }
    if local_is_dir && req.offset > 0 {
        bail!("--offset cannot be combined with a directory upload");
    }

    let (total_bytes, file_count) = if local_is_dir {
        dir_stats(&req.local).await?
    } else {
        (meta.len(), 1)
    };

    let req = Arc::new(req);
    let local_name = local_basename(&req.local);
    let transfer = Arc::new(TransferService::new(config.clone())?);
    let semaphore = Arc::new(Semaphore::new(parallel.max(1)));

    let metas: Vec<(Uuid, String, String)> = targets
        .iter()
        .map(|c| (c.id, c.name.clone(), c.host.clone()))
        .collect();
    let mut results: Vec<Option<HostUploadResult>> = vec![None; targets.len()];
    let mut set: JoinSet<(usize, HostUploadResult)> = JoinSet::new();

    for (index, conn) in targets.into_iter().enumerate() {
        let transfer = Arc::clone(&transfer);
        let req = Arc::clone(&req);
        let on_event = on_event.clone();
        let semaphore = Arc::clone(&semaphore);
        let local_name = local_name.clone();
        set.spawn(async move {
            // The semaphore is never closed, so acquire cannot fail.
            let _permit = semaphore.acquire_owned().await.ok();
            let result = run_host(
                transfer,
                conn,
                req,
                local_is_dir,
                local_name,
                total_bytes,
                file_count,
                on_event,
            )
            .await;
            (index, result)
        });
    }

    let mut aborted: Vec<String> = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((index, result)) => results[index] = Some(result),
            Err(e) => aborted.push(format!("upload task aborted: {e}")),
        }
    }

    // A panicked/aborted task leaves its slot empty; surface it as a failure
    // so the caller still gets one result per target, in order.
    for (index, slot) in results.iter_mut().enumerate() {
        if slot.is_none() {
            let error = aborted
                .pop()
                .unwrap_or_else(|| "upload task aborted".to_string());
            let (connection_id, name, host) = metas[index].clone();
            *slot = Some(HostUploadResult {
                connection_id,
                name,
                host,
                ok: false,
                error: Some(error),
                files: 0,
                bytes: 0,
                via_scp: false,
                remote_path: String::new(),
                elapsed_ms: 0,
            });
        }
    }

    Ok(results.into_iter().flatten().collect())
}

#[allow(clippy::too_many_arguments)]
async fn run_host(
    transfer: Arc<TransferService>,
    conn: Connection,
    req: Arc<UploadRequest>,
    local_is_dir: bool,
    local_name: String,
    total_bytes: u64,
    file_count: u64,
    on_event: Option<UploadEventFn>,
) -> HostUploadResult {
    let started = Instant::now();
    let emit = |event: UploadEvent| {
        if let Some(cb) = &on_event {
            cb(event);
        }
    };

    emit(UploadEvent::Started {
        connection_id: conn.id,
        name: conn.name.clone(),
        host: conn.host.clone(),
    });

    let via_scp = !transfer.has_sftp(&conn);
    let mut remote_path = req.remote.clone();
    let mut files = 0u64;
    let mut bytes = 0u64;
    let outcome: Result<(), String>;

    if via_scp {
        match transfer
            .scp_upload(&conn, &req.local, &req.remote, req.recursive)
            .await
        {
            Ok(()) => {
                files = file_count;
                bytes = total_bytes;
                outcome = Ok(());
            }
            Err(e) => outcome = Err(format!("{e:#}")),
        }
    } else {
        match transfer.open_sftp(&conn).await {
            Err(e) => outcome = Err(format!("{e:#}")),
            Ok(sftp) => {
                let dest = resolve_dest(&*sftp, &req.remote, local_is_dir, &local_name).await;
                remote_path = dest.clone();
                let progress = make_progress(&conn, &on_event);
                if local_is_dir {
                    match transfer
                        .upload_recursive_on(
                            &*sftp,
                            &req.local,
                            &dest,
                            req.mode,
                            Some(progress),
                            total_bytes,
                        )
                        .await
                    {
                        Ok((f, b)) => {
                            files = f;
                            bytes = b;
                            outcome = Ok(());
                        }
                        Err(e) => outcome = Err(format!("{e:#}")),
                    }
                } else {
                    match transfer
                        .upload_on(
                            &*sftp,
                            &conn,
                            &req.local,
                            &dest,
                            req.offset,
                            req.mode,
                            Some(progress),
                        )
                        .await
                    {
                        Ok(b) => {
                            files = 1;
                            bytes = b;
                            outcome = Ok(());
                        }
                        Err(e) => outcome = Err(format!("{e:#}")),
                    }
                }
            }
        }
    }

    let result = HostUploadResult {
        connection_id: conn.id,
        name: conn.name.clone(),
        host: conn.host.clone(),
        ok: outcome.is_ok(),
        error: outcome.err(),
        files,
        bytes,
        via_scp,
        remote_path,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };

    // Throttling may have dropped the last chunk; make sure the bar reaches
    // 100% before the host is marked finished.
    if result.ok && !via_scp {
        emit(UploadEvent::Progress {
            connection_id: conn.id,
            bytes_done: bytes,
            bytes_total: Some(total_bytes),
        });
    }

    emit(UploadEvent::Finished {
        result: result.clone(),
    });
    result
}

/// Resolve the remote destination, appending the local basename when the
/// destination is a directory. On SFTP a remote path without a trailing `/`
/// is `stat`ed: if it already exists as a directory, the file goes inside it.
async fn resolve_dest(
    sftp: &dyn SftpSession,
    remote: &str,
    local_is_dir: bool,
    local_name: &str,
) -> String {
    if !local_is_dir && !remote.ends_with('/') {
        if let Ok(entry) = sftp.stat(remote).await {
            if entry.is_dir {
                return resolve_remote_path(remote, local_name, false, true);
            }
        }
    }
    resolve_remote_path(remote, local_name, local_is_dir, false)
}

/// Build a throttled progress callback (~10 events/s per host, plus the final
/// chunk).
fn make_progress(conn: &Connection, on_event: &Option<UploadEventFn>) -> ProgressFn {
    let on_event = on_event.clone();
    let id = conn.id;
    let last = Arc::new(Mutex::new(None::<Instant>));
    Box::new(move |done, total| {
        let is_final = total.map(|t| done >= t).unwrap_or(false);
        {
            let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
            let ready = is_final
                || last
                    .map(|t| t.elapsed() >= Duration::from_millis(100))
                    .unwrap_or(true);
            if !ready {
                return;
            }
            *last = Some(Instant::now());
        }
        if let Some(cb) = &on_event {
            cb(UploadEvent::Progress {
                connection_id: id,
                bytes_done: done,
                bytes_total: total,
            });
        }
    })
}

/// Basename of a local path, for joining onto a remote directory.
pub fn local_basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolve the effective remote path for an upload.
///
/// When the destination is a directory (`remote` ends with `/`, or
/// `remote_is_dir`), a local *file* is uploaded into it under its basename.
/// A local directory (recursive upload) is copied into the destination as-is,
/// matching `TransferService::upload_recursive` semantics.
pub fn resolve_remote_path(
    remote: &str,
    local_name: &str,
    local_is_dir: bool,
    remote_is_dir: bool,
) -> String {
    let into_dir = remote.ends_with('/') || remote_is_dir;
    if !into_dir || local_is_dir {
        return remote.to_string();
    }
    let base = remote.trim_end_matches('/');
    if base.is_empty() {
        format!("/{local_name}")
    } else {
        format!("{base}/{local_name}")
    }
}

/// `(total_bytes, file_count)` for a local file or directory tree.
///
/// Regular files only; symlinks and special files are skipped, matching what
/// the uploader actually transfers. Used by callers to show a pre-flight size.
pub async fn local_stats(path: &Path) -> Result<(u64, u64)> {
    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("stat {}", path.display()))?;
    if meta.is_dir() {
        dir_stats(path).await
    } else {
        Ok((meta.len(), 1))
    }
}

/// `(total_bytes, file_count)` of every regular file under `dir`, recursively.
fn dir_stats(dir: &Path) -> BoxFuture<'_, Result<(u64, u64)>> {
    Box::pin(async move {
        let mut bytes = 0u64;
        let mut files = 0u64;
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .with_context(|| format!("read local dir {}", dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                let (b, f) = dir_stats(&entry.path()).await?;
                bytes += b;
                files += f;
            } else if file_type.is_file() {
                bytes += entry.metadata().await?.len();
                files += 1;
            }
        }
        Ok((bytes, files))
    })
}

#[cfg(test)]
mod tests {
    use super::{local_basename, resolve_remote_path};
    use std::path::Path;

    #[test]
    fn trailing_slash_joins_file_basename() {
        assert_eq!(
            resolve_remote_path("/opt/releases/", "app.tar.gz", false, false),
            "/opt/releases/app.tar.gz"
        );
        assert_eq!(
            resolve_remote_path("/", "app.tar.gz", false, false),
            "/app.tar.gz"
        );
        // No trailing slash and not known to be a dir: kept verbatim.
        assert_eq!(
            resolve_remote_path("/opt/releases/app.tar.gz", "app.tar.gz", false, false),
            "/opt/releases/app.tar.gz"
        );
    }

    #[test]
    fn existing_remote_dir_joins_file_basename() {
        assert_eq!(
            resolve_remote_path("/opt/app", "app.tar.gz", false, true),
            "/opt/app/app.tar.gz"
        );
    }

    #[test]
    fn directory_upload_keeps_destination() {
        // A recursive directory upload is copied into the destination as-is.
        assert_eq!(
            resolve_remote_path("/etc/myapp/", "myapp", true, false),
            "/etc/myapp/"
        );
        assert_eq!(
            resolve_remote_path("/etc/myapp", "myapp", true, false),
            "/etc/myapp"
        );
    }

    #[test]
    fn basename_of_relative_paths() {
        assert_eq!(
            local_basename(Path::new("./deploy.tar.gz")),
            "deploy.tar.gz"
        );
        assert_eq!(local_basename(Path::new("config/")), "config");
        assert_eq!(local_basename(Path::new("/a/b/c.bin")), "c.bin");
    }
}
