//! Native SSH transport powered by `russh`.
//!
//! Implements `SshTransport` using the pure-Rust `russh` crate.
//! Eligible connections (no kerberos, no bastion) use this path by default.

#![allow(dead_code)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey, PublicKeyBase64, PublicKeyOrCertificate};
use russh::ChannelMsg;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, warn};

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::auth;
use crate::services::known_hosts;
use crate::services::transport::types::{
    CappedBuf, CertAuth, ExecOptions, ExecOutput, LimitedOutput, PtyIo, SftpSession, ShellHandle,
    SshTransport, TransportError,
};
use std::time::Duration;

// ──────────────────────────────────────────────────────────────────────────────
// Handler — processes server-side callbacks
// ──────────────────────────────────────────────────────────────────────────────

struct ClientHandler {
    config: AppConfig,
    hostname: String,
    port: u16,
    host_key_accepted: bool,
    /// No terminal output: a first-seen host key is logged, not printed.
    quiet: bool,
}

impl ClientHandler {
    fn new(config: AppConfig, hostname: String, port: u16) -> Self {
        Self {
            config,
            hostname,
            port,
            host_key_accepted: false,
            quiet: false,
        }
    }

    fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

/// The host key blob to check against known_hosts.
///
/// There is no `@cert-authority` support, so a host certificate gets the same
/// treatment OpenSSH gives one with no matching CA: its embedded host key is
/// checked like a plain key. A certificate never bypasses known_hosts.
fn host_key_blob(server_key: &PublicKeyOrCertificate) -> Vec<u8> {
    match server_key {
        PublicKeyOrCertificate::PublicKey { key, .. } => key.public_key_bytes(),
        PublicKeyOrCertificate::Certificate(cert) => {
            PublicKey::from(cert.public_key().clone()).public_key_bytes()
        }
    }
}

impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let khf = known_hosts::default_path();
        let key_bytes = host_key_blob(server_public_key);
        // known_hosts stores the key *blob* type (`ssh-rsa`), whereas
        // `PublicKey::name()` reports the negotiated signature algorithm
        // (`rsa-sha2-512`). Using the latter made every stored RSA entry
        // miss, so a changed RSA host key was silently TOFU-accepted.
        let key_type = known_hosts::blob_key_type(&key_bytes)
            .ok_or_else(|| anyhow!("malformed host key blob from {}", self.hostname))?;

        match known_hosts::check(&khf, &self.hostname, self.port, key_type, &key_bytes)? {
            known_hosts::CheckResult::Revoked { remote_fp } => Err(anyhow!(
                "REVOKED host key presented by {}:{} (fingerprint: {remote_fp})\n\
                 The key is marked @revoked in known_hosts; refusing to connect.",
                self.hostname,
                self.port
            )),
            known_hosts::CheckResult::KnownGood => {
                debug!("Host key verified for {}:{}", self.hostname, self.port);
                self.host_key_accepted = true;
                Ok(true)
            }
            known_hosts::CheckResult::Mismatch {
                stored_fp,
                remote_fp,
            } => {
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
                        self.hostname,
                        self.port
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
                        self.hostname,
                        self.port
                    ))
                } else {
                    // accept-new — print fingerprint and persist
                    if self.quiet {
                        info!(
                            "accepting new host key for {}:{} ({key_type}, {remote_fp})",
                            self.hostname, self.port
                        );
                    } else {
                        eprintln!(
                            "The authenticity of host '{}:{}' can't be established.\n\
                             {key_type} key fingerprint is {}.\n\
                             Host key added to known_hosts.",
                            self.hostname, self.port, remote_fp
                        );
                    }
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
        self.connect_with(conn, false, None).await
    }

    async fn connect_with(
        &self,
        conn: &Connection,
        quiet: bool,
        connect_timeout: Option<Duration>,
    ) -> Result<Handle<ClientHandler>, TransportError> {
        let russh_config = Arc::new(client::Config::default());
        let handler =
            ClientHandler::new(self.config.clone(), conn.host.clone(), conn.port).quiet(quiet);
        let addr = format!("{}:{}", conn.host, conn.port);
        let connect = client::connect(russh_config, addr.as_str(), handler);
        let res = match connect_timeout {
            Some(t) => tokio::time::timeout(t, connect).await.map_err(|_| {
                TransportError::Permanent(anyhow!(
                    "connect to {addr} timed out after {}s",
                    t.as_secs()
                ))
            })?,
            None => connect.await,
        };
        res.map_err(|e| TransportError::Permanent(anyhow!("TCP/SSH connect failed: {e}")))
    }

    /// Try public-key (unencrypted), ssh-agent, then password auth.
    async fn authenticate(
        &self,
        handle: &mut Handle<ClientHandler>,
        conn: &Connection,
    ) -> Result<bool, TransportError> {
        self.authenticate_with(handle, conn, false).await
    }

    /// Like [`Self::authenticate`]. With `non_interactive` it never prompts: when keys and
    /// agent fail it returns an error that says what to do instead of asking for a password.
    async fn authenticate_with(
        &self,
        handle: &mut Handle<ClientHandler>,
        conn: &Connection,
        non_interactive: bool,
    ) -> Result<bool, TransportError> {
        let user = &conn.user;
        // Sign with SHA-2 for RSA keys when the server advertises it; `None`
        // falls back to legacy ssh-rsa (SHA-1). Ignored for non-RSA keys.
        let rsa_hash = handle
            .best_supported_rsa_hash()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("{e}")))?
            .flatten();
        let mut tried_keys = 0usize;
        for path in auth::candidate_key_paths(conn, &self.config.auth) {
            match auth::try_load_key_no_passphrase(&path) {
                Ok(Some(key)) => {
                    debug!("Trying pubkey auth with {path:?}");
                    tried_keys += 1;
                    let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
                    if handle
                        .authenticate_publickey(user, key)
                        .await
                        .map_err(|e| TransportError::Permanent(anyhow!("{e}")))?
                        .success()
                    {
                        info!("Authenticated with key {path:?}");
                        return Ok(true);
                    }
                    debug!("Key {path:?} rejected");
                }
                Ok(None) => debug!("Key {path:?} is encrypted, skipping"),
                Err(e) => debug!("Could not load key {path:?}: {e}"),
            }
        }
        let (agent_ok, tried_agent) = self.authenticate_agent(handle, user, rsa_hash).await;
        if agent_ok {
            return Ok(true);
        }
        if non_interactive {
            return Err(TransportError::Permanent(anyhow!(
                "needs interactive auth; use a key or ssh-agent \
                 (tried {tried_keys} key file(s) and {tried_agent} agent identity(ies) \
                 for {user}@{})",
                conn.host
            )));
        }
        // Password fallback. The prompt blocks on the TTY, so keep it off the
        // async worker threads; wipe our copy of the password once sent.
        let prompt = format!("{}@{}'s password: ", user, conn.host);
        let pw = tokio::task::spawn_blocking(move || rpassword::prompt_password(prompt))
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("password prompt: {e}")))?
            .map_err(|e| TransportError::Permanent(anyhow!("password prompt: {e}")))?;
        let pw = zeroize::Zeroizing::new(pw);
        handle
            .authenticate_password(user, pw.as_str())
            .await
            .map(|res| res.success())
            .map_err(|e| TransportError::Permanent(anyhow!("{e}")))
    }

    /// Offer the ssh-agent's identities one by one. Returns `(authenticated, identities
    /// tried)`. Any agent problem (no socket, refused, protocol error) counts as "no
    /// identities": the caller moves on to the next method.
    #[cfg(unix)]
    async fn authenticate_agent(
        &self,
        handle: &mut Handle<ClientHandler>,
        user: &str,
        rsa_hash: Option<russh::keys::HashAlg>,
    ) -> (bool, usize) {
        use russh::keys::agent::client::AgentClient;
        use russh::keys::agent::AgentIdentity;

        if !self.config.auth.use_agent {
            return (false, 0);
        }
        let Some(sock) = auth::agent_socket_path(&self.config.auth) else {
            debug!("No ssh-agent socket (SSH_AUTH_SOCK unset)");
            return (false, 0);
        };
        let mut agent = match AgentClient::connect_uds(&sock).await {
            Ok(a) => a,
            Err(e) => {
                debug!("Cannot reach ssh-agent at {sock:?}: {e}");
                return (false, 0);
            }
        };
        let identities = match agent.request_identities().await {
            Ok(ids) => ids,
            Err(e) => {
                debug!("ssh-agent identity list failed: {e}");
                return (false, 0);
            }
        };
        let mut tried = 0usize;
        for identity in identities {
            tried += 1;
            let res = match identity {
                AgentIdentity::PublicKey { key, comment } => {
                    debug!("Trying agent key {comment:?}");
                    let hash = matches!(key.algorithm(), russh::keys::Algorithm::Rsa { .. })
                        .then_some(rsa_hash)
                        .flatten();
                    handle
                        .authenticate_publickey_with(user, key, hash, &mut agent)
                        .await
                }
                AgentIdentity::Certificate {
                    certificate,
                    comment,
                } => {
                    debug!("Trying agent certificate {comment:?}");
                    handle
                        .authenticate_certificate_with(user, certificate, rsa_hash, &mut agent)
                        .await
                }
            };
            match res {
                Ok(r) if r.success() => {
                    info!("Authenticated with an ssh-agent identity");
                    return (true, tried);
                }
                Ok(_) => debug!("ssh-agent identity rejected"),
                Err(e) => debug!("ssh-agent signing failed: {e}"),
            }
        }
        (false, tried)
    }

    /// ssh-agent over a Unix socket only; other platforms skip it.
    #[cfg(not(unix))]
    async fn authenticate_agent(
        &self,
        _handle: &mut Handle<ClientHandler>,
        _user: &str,
        _rsa_hash: Option<russh::keys::HashAlg>,
    ) -> (bool, usize) {
        (false, 0)
    }

    /// Authenticate with an OpenSSH certificate and its private key.
    async fn authenticate_cert(
        handle: &mut Handle<ClientHandler>,
        user: &str,
        cert: &CertAuth,
    ) -> Result<bool, TransportError> {
        let key = russh::keys::PrivateKey::from_openssh(cert.private_key_openssh.as_bytes())
            .map_err(|e| TransportError::Permanent(anyhow!("certificate key: {e}")))?;
        let certificate = russh::keys::Certificate::from_openssh(&cert.certificate_openssh)
            .map_err(|e| TransportError::Permanent(anyhow!("certificate: {e}")))?;
        handle
            .authenticate_openssh_cert(user, Arc::new(key), certificate)
            .await
            .map(|res| res.success())
            .map_err(|e| TransportError::Permanent(anyhow!("{e}")))
    }

    async fn run_exec(
        &self,
        conn: &Connection,
        command: &str,
    ) -> Result<ExecOutput, TransportError> {
        let out = self
            .run_exec_limited(conn, command, &ExecOptions::default())
            .await?;
        Ok(ExecOutput {
            stdout: out.stdout,
            stderr: out.stderr,
            exit_code: out.exit_code.unwrap_or(0),
        })
    }

    async fn run_exec_limited(
        &self,
        conn: &Connection,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<LimitedOutput, TransportError> {
        let mut handle = self
            .connect_with(conn, opts.non_interactive, opts.connect_timeout)
            .await?;

        let auth_phase = async {
            let ok = match &opts.cert_auth {
                Some(cert) => Self::authenticate_cert(&mut handle, &conn.user, cert).await?,
                None => {
                    self.authenticate_with(&mut handle, conn, opts.non_interactive)
                        .await?
                }
            };
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        };
        match opts.connect_timeout {
            // Only bound the prompt-free paths: a human typing a password is not "slow".
            Some(t) if opts.non_interactive || opts.cert_auth.is_some() => {
                tokio::time::timeout(t, auth_phase).await.map_err(|_| {
                    TransportError::Permanent(anyhow!(
                        "authentication to {} timed out after {}s",
                        conn.host,
                        t.as_secs()
                    ))
                })??
            }
            _ => auth_phase.await?,
        }

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("channel open: {e}")))?;

        channel
            .exec(true, command.as_bytes())
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("exec: {e}")))?;

        let mut stdout = CappedBuf::new(opts.max_output_bytes);
        let mut stderr = CappedBuf::new(opts.max_output_bytes);
        let mut exit_code: Option<i32> = None;

        let drain = async {
            loop {
                let Some(msg) = channel.wait().await else {
                    break;
                };
                match msg {
                    ChannelMsg::Data { data } => stdout.push(&data),
                    ChannelMsg::ExtendedData { data, ext: 1 } => stderr.push(&data),
                    ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status as i32),
                    // Servers commonly send EOF *before* exit-status; keep reading
                    // until the channel closes so the exit code isn't lost.
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
        };
        let timed_out = match opts.timeout {
            Some(t) => tokio::time::timeout(t, drain).await.is_err(),
            None => {
                drain.await;
                false
            }
        };
        if timed_out {
            // Closing the channel makes sshd hang up the command's session.
            let _ = channel.close().await;
        }
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
        Ok(LimitedOutput {
            truncated: stdout.truncated() || stderr.truncated(),
            stdout: stdout.into_vec(),
            stderr: stderr.into_vec(),
            exit_code: if timed_out { None } else { exit_code },
            timed_out,
        })
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
            .request_pty(true, &term, io.cols as u32, io.rows as u32, 0, 0, &[])
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("PTY request: {e}")))?;

        channel
            .request_shell(true)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("shell request: {e}")))?;

        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let PtyIo {
            mut stdin_rx,
            output_tx,
            mut resize_rx,
            ..
        } = io;

        // Use channel.wait() for incoming data and channel.data() for outgoing,
        // following the official russh pattern (no make_reader/make_writer).
        tokio::spawn(async move {
            let mut exit_code = 0i32;
            let mut stdin_closed = false;
            let mut resize_closed = false;

            loop {
                tokio::select! {
                    // Local keystrokes → remote
                    data = stdin_rx.recv(), if !stdin_closed => {
                        match data {
                            None => {
                                stdin_closed = true;
                                let _ = channel.eof().await;
                            }
                            Some(d) => { let _ = channel.data(d.as_slice()).await; }
                        }
                    }
                    // Terminal resize. Once the sender is gone `recv()` resolves
                    // immediately forever — disable the branch or the select
                    // loop spins at 100% CPU.
                    dims = resize_rx.recv(), if !resize_closed => {
                        match dims {
                            Some((cols, rows)) => {
                                let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                            }
                            None => resize_closed = true,
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
                            // EOF may precede exit-status; wait for it or Close.
                            Some(ChannelMsg::Close) => break,
                            _ => {}
                        }
                    }
                    // External cancel signal
                    _ = &mut cancel_rx => break,
                }
            }
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
            let _ = exit_tx.send(exit_code);
        });

        Ok(ShellHandle {
            exit_rx,
            cancel: Some(cancel_tx),
        })
    }

    async fn exec(&self, conn: &Connection, command: &str) -> Result<ExecOutput, TransportError> {
        self.run_exec(conn, command).await
    }

    async fn exec_limited(
        &self,
        conn: &Connection,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<LimitedOutput, TransportError> {
        self.run_exec_limited(conn, command, opts).await
    }

    async fn open_sftp(&self, conn: &Connection) -> Result<Box<dyn SftpSession>, TransportError> {
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
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
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
            let _ = guard
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        });

        Ok(crate::services::transport::types::ForwardHandle::new(
            task, cancel_tx,
        ))
    }

    async fn forward_dynamic(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<crate::services::transport::types::ForwardHandle, TransportError> {
        use std::sync::Arc;
        use tokio::net::TcpListener;
        use tokio::sync::Mutex;
        use tracing::warn;

        let mut handle = self.connect(conn).await?;
        self.authenticate(&mut handle, conn).await.and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err(TransportError::Permanent(anyhow!("Authentication failed")))
            }
        })?;

        let listener = TcpListener::bind((bind_host, bind_port))
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("bind {bind_host}:{bind_port}: {e}")))?;

        let ssh = Arc::new(Mutex::new(handle));
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut cancel_rx => break,
                    result = listener.accept() => {
                        let Ok((stream, peer_addr)) = result else { break };
                        let ssh2 = Arc::clone(&ssh);
                        tokio::spawn(async move {
                            use crate::services::transport::socks5;
                            let mut stream = stream;
                            match socks5::handshake(&mut stream).await {
                                Ok((target_host, target_port)) => {
                                    let channel = {
                                        let guard = ssh2.lock().await;
                                        guard.channel_open_direct_tcpip(
                                            target_host,
                                            target_port as u32,
                                            peer_addr.ip().to_string(),
                                            peer_addr.port() as u32,
                                        ).await
                                    };
                                    match channel {
                                        Ok(chan) => {
                                            if socks5::send_reply(&mut stream, socks5::REP_SUCCEEDED).await.is_ok() {
                                                proxy_tcp_channel(stream, chan).await;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = socks5::send_reply(&mut stream, socks5::REP_HOST_UNREACHABLE).await;
                                            warn!("dynamic proxy: direct-tcpip failed: {e}");
                                        }
                                    }
                                }
                                Err(e) => warn!("dynamic proxy: SOCKS5 handshake failed: {e}"),
                            }
                        });
                    }
                }
            }
            let guard = ssh.lock().await;
            let _ = guard
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        });

        Ok(crate::services::transport::types::ForwardHandle::new(
            task, cancel_tx,
        ))
    }

    async fn run_interactive(
        &self,
        conn: &Connection,
        recorder: Option<crate::services::recording::SharedRecorder>,
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

        let (cols, rows) =
            size().map_err(|e| TransportError::Permanent(anyhow!("terminal size: {e}")))?;
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into());
        channel
            .request_pty(true, &term, cols as u32, rows as u32, 0, 0, &[])
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("PTY request: {e}")))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| TransportError::Permanent(anyhow!("shell request: {e}")))?;

        enable_raw_mode().map_err(|e| TransportError::Permanent(anyhow!("raw mode: {e}")))?;

        // Keystrokes come from a plain thread, not `tokio::io::stdin()`: Tokio reads stdin
        // on its blocking pool, and the read still pending when the remote shell exits kept
        // the runtime, and so the process, alive until the next key press.
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
        std::thread::spawn(move || {
            use std::io::Read;
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 4096];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if stdin_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut stdout = tokio::io::stdout();
        let mut stdin_closed = false;
        let mut exit_code = 0i32;
        let mut carry = crate::services::pty::Utf8Carry::default();
        let mut dec_buf = String::new();
        #[cfg(unix)]
        let mut sigwinch =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()).ok();

        loop {
            tokio::select! {
                data = stdin_rx.recv(), if !stdin_closed => {
                    match data {
                        None => { stdin_closed = true; let _ = channel.eof().await; }
                        Some(d) => { let _ = channel.data(d.as_slice()).await; }
                    }
                }
                _ = async {
                    #[cfg(unix)]
                    if let Some(ref mut sig) = sigwinch {
                        let _ = sig.recv().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                    #[cfg(not(unix))]
                    std::future::pending::<()>().await;
                } => {
                    if let Ok((c, r)) = size() {
                        let _ = channel.window_change(c as u32, r as u32, 0, 0).await;
                        if let Some(ref rec) = recorder {
                            if let Ok(mut g) = rec.lock() {
                                g.resize(c, r);
                            }
                        }
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        None => break,
                        Some(ChannelMsg::Data { ref data }) => {
                            let _ = stdout.write_all(data).await;
                            let _ = stdout.flush().await;
                            if let Some(ref rec) = recorder {
                                dec_buf.clear();
                                carry.decode_into(data, &mut dec_buf);
                                if !dec_buf.is_empty() {
                                    if let Ok(mut g) = rec.lock() {
                                        g.output(&dec_buf);
                                    }
                                }
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = exit_status as i32;
                            if !stdin_closed { let _ = channel.eof().await; }
                            break;
                        }
                        Some(ChannelMsg::Close) => break,
                        _ => {}
                    }
                }
            }
        }

        if let Some(ref rec) = recorder {
            dec_buf.clear();
            carry.finish_into(&mut dec_buf);
            if !dec_buf.is_empty() {
                if let Ok(mut g) = rec.lock() {
                    g.output(&dec_buf);
                }
            }
        }

        let _ = disable_raw_mode();
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
        Ok(exit_code)
    }
}

/// Bidirectionally proxy bytes between a local `TcpStream` and an SSH `direct-tcpip` channel.
async fn proxy_tcp_channel(
    stream: tokio::net::TcpStream,
    channel: russh::Channel<russh::client::Msg>,
) {
    use tokio::io::copy_bidirectional;
    let mut tcp = stream;
    let mut ssh = channel.into_stream();
    let _ = copy_bidirectional(&mut tcp, &mut ssh).await;
}
