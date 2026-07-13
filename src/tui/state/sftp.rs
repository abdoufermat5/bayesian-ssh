use crate::models::Connection;
use crate::services::transport::types::{SftpSession, SshTransport};
use crate::services::transport::{pick_kind, RusshTransport, TransportKind};
use crate::tui::models::{AppMode, ConfirmAction, FilesTabState, SftpMsg};
use crate::tui::state::App;
use anyhow::Result;

impl App {
    /// Drain completed SFTP task results and update `files_state`.
    pub fn drain_sftp_results(&mut self) {
        while let Ok(msg) = self.sftp_rx.try_recv() {
            match msg {
                SftpMsg::Listed { path, mut entries } => {
                    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                        fs.error = None;
                        fs.current_path = path;
                        fs.entries = entries;
                        fs.selected = 0;
                    }
                }
                SftpMsg::Downloaded {
                    remote,
                    local,
                    bytes,
                } => {
                    self.set_status(format!(
                        "Downloaded '{}' → '{}' ({bytes} bytes)",
                        remote, local
                    ));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                }
                SftpMsg::Uploaded {
                    local,
                    remote,
                    bytes,
                } => {
                    self.set_status(format!(
                        "Uploaded '{}' → '{}' ({bytes} bytes)",
                        local, remote
                    ));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                    // Refresh listing so the new file appears
                    if let Some(fs) = self.files_state.as_ref() {
                        let conn = fs.connection.clone();
                        let path = fs.current_path.clone();
                        self.spawn_sftp_list(conn, path);
                    }
                }
                SftpMsg::Removed { path } => {
                    self.set_status(format!("Deleted '{path}'"));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                    if let Some(fs) = self.files_state.as_ref() {
                        let conn = fs.connection.clone();
                        let dir = fs.current_path.clone();
                        self.spawn_sftp_list(conn, dir);
                    }
                }
                SftpMsg::DirCreated { path } => {
                    self.set_status(format!("Created directory '{path}'"));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                    if let Some(fs) = self.files_state.as_ref() {
                        let conn = fs.connection.clone();
                        let dir = fs.current_path.clone();
                        self.spawn_sftp_list(conn, dir);
                    }
                }
                SftpMsg::Renamed { from, to } => {
                    self.set_status(format!("Renamed '{}' → '{}'", from, to));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                    if let Some(fs) = self.files_state.as_ref() {
                        let conn = fs.connection.clone();
                        let dir = fs.current_path.clone();
                        self.spawn_sftp_list(conn, dir);
                    }
                }
                SftpMsg::Error(msg) => {
                    self.set_status(format!("SFTP error: {msg}"));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                        fs.error = Some(msg);
                    }
                }
            }
        }
    }

    /// Open the Files tab for `connection`, initiating a directory listing of "/".
    pub fn open_files_for_connection(&mut self, connection: Connection) {
        let state = FilesTabState::new(connection.clone());
        self.files_state = Some(state);
        self.spawn_sftp_list(connection, "/".to_string());
    }

    /// Navigate into the selected entry (if it is a directory).
    pub fn files_enter_selected(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading {
                return;
            }
            let entry = match fs.selected_entry() {
                Some(e) => e,
                None => return,
            };
            if !entry.is_dir {
                return;
            }
            let new_path = entry.path.to_string_lossy().into_owned();
            (fs.connection.clone(), new_path)
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Navigate to the parent directory.
    pub fn files_go_up(&mut self) {
        let (conn, parent) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading || fs.current_path == "/" {
                return;
            }
            (fs.connection.clone(), fs.parent_path())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, parent);
    }

    /// Refresh the current directory listing.
    pub fn files_refresh(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            (fs.connection.clone(), fs.current_path.clone())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Download a remote file to a user-specified local path, with pre-checks.
    pub fn files_download_to(&mut self, remote_path: String, local_dest: String) {
        let (is_loading, conn) = match self.files_state.as_ref() {
            Some(s) => (s.is_loading, s.connection.clone()),
            None => return,
        };
        if is_loading {
            return;
        }

        let local_path = std::path::PathBuf::from(&local_dest);

        // Pre-check: parent directory must exist
        if let Some(parent) = local_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                self.set_status(format!(
                    "Parent directory '{}' does not exist",
                    parent.display()
                ));
                return;
            }
            // Pre-check: parent directory must be writable
            if !parent.as_os_str().is_empty() {
                match std::fs::metadata(parent) {
                    Ok(meta) => {
                        if meta.permissions().readonly() {
                            self.set_status(format!(
                                "Directory '{}' is not writable",
                                parent.display()
                            ));
                            return;
                        }
                    }
                    Err(_) => {
                        self.set_status(format!("Cannot access directory '{}'", parent.display()));
                        return;
                    }
                }
            }
        }

        // Pre-check: warn if file already exists
        if local_path.exists() {
            self.set_status(format!(
                "Overwriting existing file '{}'…",
                local_path.display()
            ));
        }

        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
                let read_fut = sftp.read_all(&remote_path, chunk_tx);
                let mut file = tokio::fs::File::create(&local_path).await?;
                let write_fut = async {
                    let mut total = 0u64;
                    while let Some(chunk) = chunk_rx.recv().await {
                        use tokio::io::AsyncWriteExt;
                        file.write_all(&chunk).await?;
                        total += chunk.len() as u64;
                    }
                    Ok::<u64, anyhow::Error>(total)
                };
                let (read_result, write_result) = tokio::join!(read_fut, write_fut);
                read_result.map_err(|e| anyhow::anyhow!("{e}"))?;
                write_result
            };
            match result.await {
                Ok(bytes) => {
                    let _ = tx.send(SftpMsg::Downloaded {
                        remote: remote_path,
                        local: local_dest,
                        bytes,
                    });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Upload a local file to the current remote directory.
    pub fn files_upload(&mut self, local_path: String) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let filename = std::path::Path::new(&local_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| local_path.clone());
        let remote_path = format!("{}/{}", fs.current_path.trim_end_matches('/'), filename);
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                let mut file = tokio::fs::File::open(&local_path).await?;
                let meta = file.metadata().await?;
                let _size = meta.len();
                let mode = 0o644u32;
                let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
                let write_fut = sftp.write_all(&remote_path, 0, chunk_rx, mode);
                let read_fut = async {
                    let mut buf = vec![0u8; 32 * 1024];
                    let mut total = 0u64;
                    loop {
                        use tokio::io::AsyncReadExt;
                        let n = file.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        chunk_tx
                            .send(buf[..n].to_vec())
                            .await
                            .map_err(|_| anyhow::anyhow!("upload channel closed"))?;
                    }
                    drop(chunk_tx);
                    Ok::<u64, anyhow::Error>(total)
                };
                let (write_result, _read_result) = tokio::join!(write_fut, read_fut);
                write_result.map_err(|e| anyhow::anyhow!("{e}"))
            };
            match result.await {
                Ok(bytes) => {
                    let _ = tx.send(SftpMsg::Uploaded {
                        local: local_path,
                        remote: remote_path,
                        bytes,
                    });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Recursively download a remote directory to a local path.
    pub fn files_download_dir_to(&mut self, remote_path: String, local_dest: String) {
        let conn = match self.files_state.as_ref() {
            Some(s) => s.connection.clone(),
            None => return,
        };
        let local_dir = std::path::PathBuf::from(&local_dest);
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result: Result<(u64, u64), anyhow::Error> = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                download_dir_recursive(&*sftp, &remote_path, &local_dir).await
            }
            .await;
            match result {
                Ok((files, bytes)) => {
                    let _ = tx.send(SftpMsg::Downloaded {
                        remote: remote_path,
                        local: local_dest,
                        bytes,
                    });
                    let _ = tx.send(SftpMsg::Error(format!(
                        "Downloaded {files} files ({bytes} bytes)"
                    )));
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Recursively upload a local directory to the current remote directory.
    pub fn files_upload_dir(&mut self, local_path: String) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let dirname = std::path::Path::new(&local_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| local_path.clone());
        let remote_path = format!("{}/{}", fs.current_path.trim_end_matches('/'), dirname);
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        let local_dir = std::path::PathBuf::from(&local_path);
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result: Result<(u64, u64), anyhow::Error> = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                upload_dir_recursive(&*sftp, &local_dir, &remote_path).await
            }
            .await;
            match result {
                Ok((files, bytes)) => {
                    let _ = tx.send(SftpMsg::Uploaded {
                        local: local_path,
                        remote: remote_path,
                        bytes,
                    });
                    let _ = tx.send(SftpMsg::Error(format!(
                        "Uploaded {files} files ({bytes} bytes)"
                    )));
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Delete the selected remote entry (file or empty directory).
    pub fn files_delete_selected(&mut self) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let entry = match fs.selected_entry() {
            Some(e) => e,
            None => return,
        };
        let path = entry.path.to_string_lossy().into_owned();
        self.mode = AppMode::Confirm(ConfirmAction::DeleteFile(path));
    }

    /// Execute the confirmed remote delete.
    pub fn files_do_delete(&mut self, path: String) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                sftp.remove(&path).await.map_err(|e| anyhow::anyhow!("{e}"))
            };
            match result.await {
                Ok(()) => {
                    let _ = tx.send(SftpMsg::Removed { path });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Create a new directory under the current remote path.
    pub fn files_mkdir(&mut self, name: String) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let new_path = format!("{}/{}", fs.current_path.trim_end_matches('/'), name.trim());
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                sftp.mkdir(&new_path, 0o755)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            };
            match result.await {
                Ok(()) => {
                    let _ = tx.send(SftpMsg::DirCreated { path: new_path });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Rename the selected remote entry to `new_name` (basename only).
    pub fn files_rename(&mut self, new_name: String) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let entry = match fs.selected_entry() {
            Some(e) => e,
            None => return,
        };
        let old_path = entry.path.to_string_lossy().into_owned();
        let parent = std::path::Path::new(&old_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".to_string());
        let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name.trim());
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                sftp.rename(&old_path, &new_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            };
            match result.await {
                Ok(()) => {
                    let _ = tx.send(SftpMsg::Renamed {
                        from: old_path,
                        to: new_path,
                    });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    #[allow(dead_code)]
    pub(crate) fn refresh_sftp_dir_async(&self, path: String) {
        if let Some(ref fs) = self.files_state {
            self.spawn_sftp_list(fs.connection.clone(), path);
        }
    }

    /// Internal: spawn a task that lists `path` on `connection` via SFTP.
    fn spawn_sftp_list(&self, conn: Connection, path: String) {
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                match kind {
                    TransportKind::Native => RusshTransport::new(config.clone())
                        .open_sftp(&conn)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                        .list(&path)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}")),
                    TransportKind::Subprocess => Err(anyhow::anyhow!(
                        "SFTP not available via subprocess transport; \
                         use CLI commands: bssh upload / bssh download (SCP fallback)"
                    )),
                }
            };
            match result.await {
                Ok(entries) => {
                    let _ = tx.send(SftpMsg::Listed { path, entries });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }
}

/// Recursively download a remote directory.
///
/// Returns `(files_transferred, total_bytes)`.
#[allow(clippy::type_complexity)]
fn download_dir_recursive<'a>(
    sftp: &'a dyn SftpSession,
    remote_dir: &'a str,
    local_dir: &'a std::path::Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(u64, u64)>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(local_dir).await?;
        let entries = sftp
            .list(remote_dir)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut file_count = 0u64;
        let mut total_bytes = 0u64;

        for entry in entries {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            let remote_child = format!("{}/{}", remote_dir.trim_end_matches('/'), entry.name);
            let local_child = local_dir.join(&entry.name);

            if entry.is_dir {
                let (fc, tb) = download_dir_recursive(sftp, &remote_child, &local_child).await?;
                file_count += fc;
                total_bytes += tb;
            } else if !entry.is_symlink {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
                let remote_owned = remote_child.clone();
                let read_fut = sftp.read_all(&remote_owned, tx);

                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&local_child)
                    .await?;

                let write_fut = async {
                    let mut received = 0u64;
                    while let Some(chunk) = rx.recv().await {
                        use tokio::io::AsyncWriteExt;
                        file.write_all(&chunk).await?;
                        received += chunk.len() as u64;
                    }
                    Ok::<u64, anyhow::Error>(received)
                };

                let (read_res, write_res) = tokio::join!(read_fut, write_fut);
                read_res.map_err(|e| anyhow::anyhow!("{e}"))?;
                let received = write_res?;
                file_count += 1;
                total_bytes += received;
            }
        }

        Ok((file_count, total_bytes))
    })
}

/// Recursively upload a local directory to a remote directory.
///
/// Returns `(files_transferred, total_bytes)`.
#[allow(clippy::type_complexity)]
fn upload_dir_recursive<'a>(
    sftp: &'a dyn SftpSession,
    local_dir: &'a std::path::Path,
    remote_dir: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(u64, u64)>> + Send + 'a>> {
    Box::pin(async move {
        // Create remote directory (ignore already-exists errors)
        let _ = sftp.mkdir(remote_dir, 0o755).await;

        let mut entries = tokio::fs::read_dir(local_dir).await?;
        let mut file_count = 0u64;
        let mut total_bytes = 0u64;

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let local_child = entry.path();
            let remote_child = format!("{}/{}", remote_dir.trim_end_matches('/'), name_str);

            if file_type.is_dir() {
                let (fc, tb) = upload_dir_recursive(sftp, &local_child, &remote_child).await?;
                file_count += fc;
                total_bytes += tb;
            } else if file_type.is_file() {
                let mut file = tokio::fs::File::open(&local_child).await?;
                let mode = 0o644u32;
                let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
                let write_fut = sftp.write_all(&remote_child, 0, chunk_rx, mode);
                let read_fut = async {
                    let mut buf = vec![0u8; 32 * 1024];
                    let mut total = 0u64;
                    loop {
                        use tokio::io::AsyncReadExt;
                        let n = file.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        chunk_tx
                            .send(buf[..n].to_vec())
                            .await
                            .map_err(|_| anyhow::anyhow!("upload channel closed"))?;
                    }
                    drop(chunk_tx);
                    Ok::<u64, anyhow::Error>(total)
                };
                let (write_res, _read_res) = tokio::join!(write_fut, read_fut);
                let written = write_res.map_err(|e| anyhow::anyhow!("{e}"))?;
                file_count += 1;
                total_bytes += written;
            }
            // Skip symlinks
        }

        Ok((file_count, total_bytes))
    })
}
