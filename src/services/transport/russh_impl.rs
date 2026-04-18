//! Native SSH transport powered by `russh`.
//!
//! Implements `SshTransport` using the pure-Rust `russh` crate.
//! Eligible connections (no kerberos, no bastion) use this path by default.

#![allow(dead_code)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::ChannelMsg;
use russh_keys::key::{KeyPair, PublicKey};
use russh_keys::PublicKeyBase64;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, warn};

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::auth;
use crate::services::known_hosts;
use crate::services::transport::types::{
    ExecOutput, PtyIo, ShellHandle, SftpSession, SshTransport, TransportError,
};

// ──────────────────────────────────────────────────────────────────────────────
// Handler — processes server-side callbacks
// ──────────────────────────────────────────────────────────────────────────────

struct ClientHandler {
    config: AppConfig,
    hostname: String,
    port: u16,
    host_key_accepted: bool,
}

impl ClientHandler {
    fn new(config: AppConfig, hostname: String, port: u16) -> Self {
        Self { config, hostname, port, host_key_accepted: false }
    }
}

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let khf = known_hosts::default_path();
        let key_type = server_public_key.name();
        let key_bytes = server_public_key.public_key_bytes();

        match known_hosts::check(&khf, &self.hostname, self.port, key_type, &key_bytes)? {
            known_hosts::CheckResult::KnownGood => {
                debug!("Host key verified for {}:{}", self.hostname, self.port);
                self.host_key_accepted = true;
                Ok(true)
            }
            known_hosts::CheckResult::Mismatch { stored_fp, remote_fp } => {
                let policy = &self.config.transport.strict_host_key_checking;
                if policy == "off" {
                    warn!(
                        "Host key mismatch for {} (stored={stored_fp}, remote={remote_fp}) — \
                         strict_host_key_checking=off, continuing",
                        self.hostname
                    );
                    self.host_key_accepted = true;
                    Ok(true)
                } else {
                    Err(anyhow!(
                        "HOST KEY CHANGED for {}:{}\n\
                         Stored  : {stored_fp}\n\
                         Remote  : {remote_fp}\n\
                         This could indicate a man-in-the-middle attack.\n\
                         Remove the old key from known_hosts if this is expected.",
                        self.hostname, self.port
                    ))
                }
            }
            known_hosts::CheckResult::Unknown => {
                let policy = &self.config.transport.strict_host_key_checking;
                let remote_fp = known_hosts::fingerprint_sha256(&key_bytes);
                if policy == "strict" {
                    Err(anyhow!(
                        "Unknown host {}:{} (fingerprint: {remote_fp})\n\
                         Refusing under strict_host_key_checking=strict.",
                        self.hostname, self.port
                    ))
                } else {
                    // accept-new — print fingerprint and persist
                    eprintln!(
                        "The authenticity of host '{}:{}' can't be established.\n\
                         ED25519 key fingerprint is {}.\n\
                         Host key added to known_hosts.",
                        self.hostname, self.port, remote_fp
                    );
                    known_hosts::append(&khf, &self.hostname, self.port, key_type, &key_bytes)?;
                    self.host_key_accepted = true;
                    Ok(true)
                }
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// RusshTransport
// ──────────────────────────────────────────────────────────────────────────────

pub struct RusshTransport {
    config: AppConfig,
}

impl RusshTransport {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    async fn connect(&self, conn: &Connection) -> Result<Handle<ClientHandler>, TransportError> {
        let russh_config = Arc::new(client::Config::default());
        let handler = ClientHandler::new(self.config.clone(), conn.host.clone(), conn.port);
        let addr = format!("{}:{}", conn.host, conn.port);
        client::connect(russh_config, addr.as_str(), handler)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("TCP/SSH connect failed: {e}")))
    }

    /// Try public-key (unencrypted) then password auth.
    async fn authenticate(
        &self,
        handle: &mut Handle<ClientHandler>,
        conn: &Connection,
    ) -> Result<bool, TransportError> {
        let user = &conn.user;
        for path in auth::candidate_key_paths(conn, &self.config.auth) {
            match auth::try_load_key_no_passphrase(&path) {
                Ok(Some(kp)) => {
                    debug!("Trying pubkey auth with {path:?}");
                    match handle
                        .authenticate_publickey(user, Arc::new(kp))
                        .await
                        .map_err(|e| TransportError::Permanent(anyhow!("{e}")))?
                    {
                        true => {
                            info!("Authenticated with key {path:?}");
                            return Ok(true);
                        }
                        false => debug!("Key {path:?} rejected"),
                    }
                }
                Ok(None) => debug!("Key {path:?} is encrypted, skipping"),
                Err(e) => debug!("Could not load key {path:?}: {e}"),
            }
        }
        // Password fallback
        let pw = rpassword::prompt_password(format!("{}@{}'s password: ", user, conn.host))
            .map_err(|e| TransportError::Permanent(anyhow!("password prompt: {e}")))?;
        handle
            .authenticate_password(user, &pw)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("{e}")))
    }

    async fn run_exec(&self, conn: &Connection, command: &str) -> Result<ExecOutput, TransportError> {
        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        })?;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("channel open: {e}")))?;

        channel
            .exec(true, command.as_bytes())
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("exec: {e}")))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = 0i32;

        loop {
            let Some(msg) = channel.wait().await else { break };
            match msg {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => exit_code = exit_status as i32,
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok(ExecOutput { stdout, stderr, exit_code })
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// SshTransport impl
// ──────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl SshTransport for RusshTransport {
    /// Open an interactive shell, bridging PtyIo channels to the SSH channel.
    async fn open_shell(
        &self,
        conn: &Connection,
        io: PtyIo,
    ) -> Result<ShellHandle, TransportError> {
        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        })?;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("channel open: {e}")))?;

        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into());
        channel
            .request_pty(
                true, &term,
                io.cols as u32, io.rows as u32,
                0, 0, &[],
            )
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("PTY request: {e}")))?;

        channel
            .request_shell(true)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("shell request: {e}")))?;

        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let PtyIo { mut stdin_rx, output_tx, mut resize_rx, .. } = io;

        // Use channel.wait() for incoming data and channel.data() for outgoing,
        // following the official russh pattern (no make_reader/make_writer).
        tokio::spawn(async move {
            let mut exit_code = 0i32;
            let mut stdin_closed = false;

            loop {
                tokio::select! {
                    // Keystrokes from TUI → remote
                    data = stdin_rx.recv(), if !stdin_closed => {
                        match data {
                            None => {
                                stdin_closed = true;
                                let _ = channel.eof().await;
                            }
                            Some(d) => { let _ = channel.data(d.as_slice()).await; }
                        }
                    }
                    // Terminal resize
                    dims = resize_rx.recv() => {
                        if let Some((cols, rows)) = dims {
                            let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                        }
                    }
                    // Data from remote and control messages
                    msg = channel.wait() => {
                        match msg {
                            None => break,
                            Some(ChannelMsg::Data { ref data }) => {
                                let _ = output_tx.send(data.to_vec()).await;
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                exit_code = exit_status as i32;
                                if !stdin_closed {
                                    let _ = channel.eof().await;
                                }
                                break;
                            }
                            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                            _ => {}
                        }
                    }
                    // External cancel signal
                    _ = &mut cancel_rx => break,
                }
            }
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
            let _ = exit_tx.send(exit_code);
        });

        Ok(ShellHandle { exit_rx, cancel: Some(cancel_tx) })
    }

    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
    ) -> Result<ExecOutput, TransportError> {
        self.run_exec(conn, command).await
    }

    async fn open_sftp(
        &self,
        conn: &Connection,
    ) -> Result<Box<dyn SftpSession>, TransportError> {
        use crate::services::transport::sftp_impl::RusshSftpSession;

        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        })?;

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("channel open: {e}")))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("sftp subsystem: {e}")))?;

        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("sftp init: {e}")))?;

        Ok(Box::new(RusshSftpSession::new(sftp)))
    }

    fn name(&self) -> &'static str {
        "native-russh"
    }

    async fn forward_local(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        remote_host: &str,
        remote_port: u16,
    ) -> Result<crate::services::transport::types::ForwardHandle, TransportError> {
        use std::sync::Arc;
        use tokio::net::TcpListener;
        use tokio::sync::Mutex;
        use tracing::warn;

        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok { Ok(()) } else { Err(TransportError::Permanent(anyhow!("Authentication failed"))) }
        })?;

        let listener = TcpListener::bind((bind_host, bind_port))
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("bind {bind_host}:{bind_port}: {e}")))?;

        // Wrap handle in Arc<Mutex> so it can be shared across per-connection tasks.
        let ssh = Arc::new(Mutex::new(handle));
        let remote_host = remote_host.to_string();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut cancel_rx => break,
                    result = listener.accept() => {
                        let Ok((stream, peer_addr)) = result else { break };
                        let ssh2 = Arc::clone(&ssh);
                        let rhost = remote_host.clone();
                        tokio::spawn(async move {
                            let channel = {
                                let guard = ssh2.lock().await;
                                guard.channel_open_direct_tcpip(
                                    rhost,
                                    remote_port as u32,
                                    peer_addr.ip().to_string(),
                                    peer_addr.port() as u32,
                                ).await
                            };
                            match channel {
                                Ok(chan) => proxy_tcp_channel(stream, chan).await,
                                Err(e) => warn!("direct-tcpip open failed: {e}"),
                            }
                        });
                    }
                }
            }
            let guard = ssh.lock().await;
            let _ = guard.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        });

        Ok(crate::services::transport::types::ForwardHandle::new(task, cancel_tx))
    }
}

/// Bidirectionally proxy bytes between a local `TcpStream` and an SSH `direct-tcpip` channel.
async fn proxy_tcp_channel(stream: tokio::net::TcpStream, channel: russh::Channel<russh::client::Msg>) {
    use tokio::io::copy_bidirectional;
    let mut tcp = stream;
    let mut ssh = channel.into_stream();
    let _ = copy_bidirectional(&mut tcp, &mut ssh).await;
}

impl RusshTransport {
    /// Drive an interactive shell session directly on the calling terminal (CLI path).
    /// Sets the terminal to raw mode, bridges stdin/stdout to the SSH channel, then
    /// restores the terminal on exit.  Returns the remote exit code.
    pub async fn run_interactive(
        &self,
        conn: &Connection,
    ) -> Result<i32, TransportError> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
        use tokio::io::AsyncWriteExt;

        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        })?;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("channel open: {e}")))?;

        let (cols, rows) = size()
            .map_err(|e| TransportError::Permanent(anyhow!("terminal size: {e}")))?;
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into());
        channel
            .request_pty(true, &term, cols as u32, rows as u32, 0, 0, &[])
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("PTY request: {e}")))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("shell request: {e}")))?;

        enable_raw_mode()
            .map_err(|e| TransportError::Permanent(anyhow!("raw mode: {e}")))?;

        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut buf = [0u8; 4096];
        let mut stdin_closed = false;
        let mut exit_code = 0i32;

        loop {
            tokio::select! {
                r = stdin.read(&mut buf), if !stdin_closed => {
                    match r {
                        Ok(0) | Err(_) => { stdin_closed = true; let _ = channel.eof().await; }
                        Ok(n) => { let _ = channel.data(&buf[..n]).await; }
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        None => break,
                        Some(ChannelMsg::Data { ref data }) => {
                            let _ = stdout.write_all(data).await;
                            let _ = stdout.flush().await;
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = exit_status as i32;
                            if !stdin_closed { let _ = channel.eof().await; }
                            break;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                        _ => {}
                    }
                }
            }
        }

        let _ = disable_raw_mode();
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok(exit_code)
    }
}
