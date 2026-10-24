#![allow(dead_code)]

use crate::models::Connection;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// One-shot command execution output.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Certificate credentials that replace the connection's normal auth for one call
/// (bssh AI "certified" mode: a per-command certificate for `bssh-agent`).
///
/// The login user is `conn.user`; the caller sets it to the certificate's principal.
#[derive(Clone)]
pub struct CertAuth {
    /// OpenSSH-format private key matching the certificate.
    pub private_key_openssh: zeroize::Zeroizing<String>,
    /// OpenSSH-format certificate (`ssh-ed25519-cert-v01@openssh.com AAAA...`).
    pub certificate_openssh: String,
}

impl std::fmt::Debug for CertAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("CertAuth").finish_non_exhaustive()
    }
}

/// Limits and auth mode for [`SshTransport::exec_limited`]. `Default` means "no limits,
/// interactive auth allowed", which is exactly what [`SshTransport::exec`] does.
#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    /// Wall-clock limit for the remote command. On expiry the channel is closed (native)
    /// or the `ssh` child is killed (subprocess) and the result is marked timed out.
    pub timeout: Option<Duration>,
    /// Keep at most this many bytes per stream (stdout and stderr each); the rest is
    /// drained and dropped, and the result is marked truncated.
    pub max_output_bytes: Option<usize>,
    /// Limit for the TCP connect and authentication phase.
    pub connect_timeout: Option<Duration>,
    /// Never prompt on a TTY (password, passphrase, host key): fail with a clear error.
    pub non_interactive: bool,
    /// Authenticate with this certificate instead of keys, agent and password.
    pub cert_auth: Option<CertAuth>,
}

impl ExecOptions {
    /// True when nothing differs from a plain `exec`.
    pub fn is_plain(&self) -> bool {
        self.timeout.is_none()
            && self.max_output_bytes.is_none()
            && self.connect_timeout.is_none()
            && !self.non_interactive
            && self.cert_auth.is_none()
    }
}

/// Result of [`SshTransport::exec_limited`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LimitedOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` when the command timed out, or ended without an exit status.
    pub exit_code: Option<i32>,
    /// A stream hit `max_output_bytes`; the tail was dropped.
    pub truncated: bool,
    pub timed_out: bool,
}

impl LimitedOutput {
    /// Apply the caps to an already-collected output (used by the default
    /// `exec_limited` and by paths that cannot stream).
    pub fn from_output(out: ExecOutput, max_output_bytes: Option<usize>) -> Self {
        let mut stdout = CappedBuf::new(max_output_bytes);
        let mut stderr = CappedBuf::new(max_output_bytes);
        stdout.push(&out.stdout);
        stderr.push(&out.stderr);
        Self {
            truncated: stdout.truncated() || stderr.truncated(),
            stdout: stdout.into_vec(),
            stderr: stderr.into_vec(),
            exit_code: Some(out.exit_code),
            timed_out: false,
        }
    }
}

/// A byte buffer that keeps the first `cap` bytes and counts the rest as dropped,
/// so a chatty command cannot grow memory without bound.
#[derive(Debug, Default)]
pub struct CappedBuf {
    buf: Vec<u8>,
    cap: Option<usize>,
    truncated: bool,
}

impl CappedBuf {
    pub fn new(cap: Option<usize>) -> Self {
        Self {
            buf: Vec::new(),
            cap,
            truncated: false,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        let Some(cap) = self.cap else {
            self.buf.extend_from_slice(data);
            return;
        };
        let room = cap.saturating_sub(self.buf.len());
        if data.len() > room {
            self.truncated = true;
        }
        self.buf.extend_from_slice(&data[..room.min(data.len())]);
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
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
// `#[async_trait]` expands to `#[must_use]` on boxed futures; newer clippy flags that.
#[allow(clippy::double_must_use)]
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
    /// `on_chunk` is called for every chunk just written to the remote;
    /// implementations must not block inside the callback.
    async fn write_all(
        &self,
        path: &str,
        offset: u64,
        source: mpsc::Receiver<Vec<u8>>,
        mode: u32,
        on_chunk: Option<Box<dyn Fn(usize) + Send + Sync>>,
    ) -> Result<u64, TransportError>;
}

/// Top-level SSH transport.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait SshTransport: Send + Sync {
    async fn open_shell(&self, conn: &Connection, io: PtyIo)
        -> Result<ShellHandle, TransportError>;

    async fn exec(&self, conn: &Connection, command: &str) -> Result<ExecOutput, TransportError>;

    /// Run a command under [`ExecOptions`]: timeout, per-stream output caps,
    /// non-interactive auth, certificate auth. [`ExecOptions::default`] behaves like
    /// [`SshTransport::exec`].
    ///
    /// The default implementation wraps `exec`: it can enforce the timeout (by dropping
    /// the future) and the caps (after the fact), but not the auth options, so it refuses
    /// those. The real transports override it.
    async fn exec_limited(
        &self,
        conn: &Connection,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<LimitedOutput, TransportError> {
        if opts.non_interactive || opts.cert_auth.is_some() {
            return Err(TransportError::permanent(anyhow::anyhow!(
                "{} transport does not support non-interactive or certificate auth",
                self.name()
            )));
        }
        let run = self.exec(conn, command);
        let out = match opts.timeout {
            Some(t) => match tokio::time::timeout(t, run).await {
                Ok(res) => res?,
                Err(_) => {
                    return Ok(LimitedOutput {
                        timed_out: true,
                        ..LimitedOutput::default()
                    })
                }
            },
            None => run.await?,
        };
        Ok(LimitedOutput::from_output(out, opts.max_output_bytes))
    }

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

    /// Drive an interactive shell session directly on the calling terminal.
    async fn run_interactive(
        &self,
        conn: &Connection,
        recorder: Option<crate::services::recording::SharedRecorder>,
    ) -> Result<i32, TransportError>;

    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_buf_keeps_the_first_bytes() {
        let mut b = CappedBuf::new(Some(5));
        b.push(b"abc");
        assert!(!b.truncated());
        b.push(b"defgh");
        assert!(b.truncated());
        b.push(b"more");
        assert_eq!(b.into_vec(), b"abcde");
    }

    #[test]
    fn capped_buf_exactly_at_the_cap_is_not_truncated() {
        let mut b = CappedBuf::new(Some(4));
        b.push(b"ab");
        b.push(b"cd");
        assert!(!b.truncated());
        assert_eq!(b.into_vec(), b"abcd");
    }

    #[test]
    fn capped_buf_without_cap_keeps_everything() {
        let mut b = CappedBuf::new(None);
        b.push(&[7u8; 10_000]);
        assert!(!b.truncated());
        assert_eq!(b.into_vec().len(), 10_000);
    }

    #[test]
    fn from_output_caps_each_stream_on_its_own() {
        let out = LimitedOutput::from_output(
            ExecOutput {
                stdout: vec![b'o'; 10],
                stderr: b"e".to_vec(),
                exit_code: 2,
            },
            Some(4),
        );
        assert_eq!(out.stdout, b"oooo");
        assert_eq!(out.stderr, b"e");
        assert!(out.truncated);
        assert_eq!(out.exit_code, Some(2));
    }

    #[test]
    fn default_options_are_plain() {
        assert!(ExecOptions::default().is_plain());
        let limited = ExecOptions {
            timeout: Some(Duration::from_secs(1)),
            ..ExecOptions::default()
        };
        assert!(!limited.is_plain());
        let batch = ExecOptions {
            non_interactive: true,
            ..ExecOptions::default()
        };
        assert!(!batch.is_plain());
    }

    #[test]
    fn cert_auth_debug_does_not_print_the_key() {
        let c = CertAuth {
            private_key_openssh: zeroize::Zeroizing::new("SECRET-KEY-MATERIAL".into()),
            certificate_openssh: "ssh-ed25519-cert-v01@openssh.com AAAA".into(),
        };
        assert!(!format!("{c:?}").contains("SECRET"));
    }

    /// A transport whose `exec` takes `delay` and returns fixed output; everything else
    /// is unused by the default `exec_limited`.
    struct Fake {
        delay: Duration,
        out: ExecOutput,
    }

    #[async_trait]
    impl SshTransport for Fake {
        async fn open_shell(
            &self,
            _: &Connection,
            _: PtyIo,
        ) -> Result<ShellHandle, TransportError> {
            Err(TransportError::permanent(anyhow::anyhow!("unused")))
        }
        async fn exec(&self, _: &Connection, _: &str) -> Result<ExecOutput, TransportError> {
            tokio::time::sleep(self.delay).await;
            Ok(self.out.clone())
        }
        async fn open_sftp(&self, _: &Connection) -> Result<Box<dyn SftpSession>, TransportError> {
            Err(TransportError::permanent(anyhow::anyhow!("unused")))
        }
        async fn forward_local(
            &self,
            _: &Connection,
            _: &str,
            _: u16,
            _: &str,
            _: u16,
        ) -> Result<ForwardHandle, TransportError> {
            Err(TransportError::permanent(anyhow::anyhow!("unused")))
        }
        async fn forward_dynamic(
            &self,
            _: &Connection,
            _: &str,
            _: u16,
        ) -> Result<ForwardHandle, TransportError> {
            Err(TransportError::permanent(anyhow::anyhow!("unused")))
        }
        async fn run_interactive(
            &self,
            _: &Connection,
            _: Option<crate::services::recording::SharedRecorder>,
        ) -> Result<i32, TransportError> {
            Err(TransportError::permanent(anyhow::anyhow!("unused")))
        }
        fn name(&self) -> &'static str {
            "fake"
        }
    }

    fn conn() -> Connection {
        Connection::new(
            "c".into(),
            "h.example".into(),
            "u".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    fn fake(delay_ms: u64) -> Fake {
        Fake {
            delay: Duration::from_millis(delay_ms),
            out: ExecOutput {
                stdout: b"hello world".to_vec(),
                stderr: Vec::new(),
                exit_code: 0,
            },
        }
    }

    #[tokio::test]
    async fn default_exec_limited_times_out() {
        let opts = ExecOptions {
            timeout: Some(Duration::from_millis(50)),
            ..ExecOptions::default()
        };
        let out = fake(5_000).exec_limited(&conn(), "x", &opts).await.unwrap();
        assert!(out.timed_out);
        assert_eq!(out.exit_code, None);
    }

    #[tokio::test]
    async fn default_exec_limited_caps_output() {
        let opts = ExecOptions {
            max_output_bytes: Some(5),
            ..ExecOptions::default()
        };
        let out = fake(0).exec_limited(&conn(), "x", &opts).await.unwrap();
        assert_eq!(out.stdout, b"hello");
        assert!(out.truncated && !out.timed_out);
        assert_eq!(out.exit_code, Some(0));
    }

    #[tokio::test]
    async fn default_exec_limited_refuses_what_it_cannot_enforce() {
        let opts = ExecOptions {
            non_interactive: true,
            ..ExecOptions::default()
        };
        assert!(fake(0).exec_limited(&conn(), "x", &opts).await.is_err());
    }
}
