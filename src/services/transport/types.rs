#![allow(dead_code)]

use crate::models::Connection;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

/// One-shot command execution output.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Bidirectional PTY channels for interactive shells.
pub struct PtyIo {
    pub rows: u16,
    pub cols: u16,
    pub stdin_rx: mpsc::Receiver<Vec<u8>>,
    pub output_tx: mpsc::Sender<Vec<u8>>,
    pub resize_rx: mpsc::Receiver<(u16, u16)>,
}

/// Handle returned from `open_shell`; drop to request cancellation.
pub struct ShellHandle {
    pub exit_rx: oneshot::Receiver<i32>,
    pub cancel: Option<oneshot::Sender<()>>,
}

impl ShellHandle {
    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel.take() {
            let _ = tx.send(());
        }
    }
}

/// Handle to an active local-forward tunnel.
///
/// Dropping the handle does NOT stop the tunnel — call [`ForwardHandle::cancel`] explicitly.
pub struct ForwardHandle {
    task: tokio::task::JoinHandle<()>,
    cancel_tx: Option<oneshot::Sender<()>>,
}

impl ForwardHandle {
    pub fn new(task: tokio::task::JoinHandle<()>, cancel_tx: oneshot::Sender<()>) -> Self {
        Self {
            task,
            cancel_tx: Some(cancel_tx),
        }
    }

    /// Signal the tunnel to stop and await clean shutdown.
    pub async fn cancel(mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

/// Errors classified by fallback policy.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Retry on the other transport (missing feature, library limitation).
    #[error("transport fallback requested: {0}")]
    Fallback(#[source] anyhow::Error),
    /// Do not retry — auth failure, host-key mismatch, network error.
    #[error("transport error: {0}")]
    Permanent(#[source] anyhow::Error),
}

impl TransportError {
    pub fn fallback(err: impl Into<anyhow::Error>) -> Self {
        Self::Fallback(err.into())
    }
    pub fn permanent(err: impl Into<anyhow::Error>) -> Self {
        Self::Permanent(err.into())
    }
}

/// Remote-side directory entry surfaced by SFTP.
#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mode: u32,
    pub modified_unix: Option<i64>,
}

/// SFTP session abstraction. Subprocess transport returns `Unsupported`.
#[async_trait]
pub trait SftpSession: Send + Sync {
    async fn list(&self, path: &str) -> Result<Vec<RemoteEntry>, TransportError>;
    async fn stat(&self, path: &str) -> Result<RemoteEntry, TransportError>;
    async fn mkdir(&self, path: &str, mode: u32) -> Result<(), TransportError>;
    async fn remove(&self, path: &str) -> Result<(), TransportError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), TransportError>;
    async fn chmod(&self, path: &str, mode: u32) -> Result<(), TransportError>;
    /// Stream-based read. Caller owns chunking and progress accounting.
    async fn read_all(
        &self,
        path: &str,
        sink: mpsc::Sender<Vec<u8>>,
    ) -> Result<u64, TransportError>;
    /// Stream-based write. `offset` supports resume; pass 0 for fresh write.
    async fn write_all(
        &self,
        path: &str,
        offset: u64,
        source: mpsc::Receiver<Vec<u8>>,
        mode: u32,
    ) -> Result<u64, TransportError>;
}

/// Top-level SSH transport.
#[async_trait]
pub trait SshTransport: Send + Sync {
    async fn open_shell(&self, conn: &Connection, io: PtyIo)
        -> Result<ShellHandle, TransportError>;

    async fn exec(&self, conn: &Connection, command: &str) -> Result<ExecOutput, TransportError>;

    async fn open_sftp(&self, conn: &Connection) -> Result<Box<dyn SftpSession>, TransportError>;

    /// Open a local TCP port forward: connections to `bind_host:bind_port` are
    /// tunnelled through SSH to `remote_host:remote_port`.
    async fn forward_local(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        remote_host: &str,
        remote_port: u16,
    ) -> Result<ForwardHandle, TransportError>;

    /// Open a SOCKS5 dynamic proxy: the local listener at `bind_host:bind_port`
    /// acts as a SOCKS5 server; each CONNECT request is forwarded through the
    /// SSH tunnel via `direct-tcpip`.
    async fn forward_dynamic(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<ForwardHandle, TransportError>;

    fn name(&self) -> &'static str;
}
