//! The [`SshTransport`] implementation over the `ssh`/`scp` binaries.

//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::models::Connection;
use async_trait::async_trait;
use std::process::Stdio;

use super::argv::{prepare_ssh_command, ArgvExtras};
use super::interactive::{
    cert_through_kerberos_bastion, needs_tty, prepare_control_dir, require_kerberos_ticket,
};
use super::process::{classify_ssh_failure, run_argv_limited, write_cert_files};
use super::SubprocessTransport;
use crate::services::transport::types::{
    ExecOptions, ExecOutput, LimitedOutput, PtyIo, SftpSession, ShellHandle, SshTransport,
    TransportError,
};

#[async_trait]
impl SshTransport for SubprocessTransport {
    async fn open_shell(
        &self,
        _conn: &Connection,
        _io: PtyIo,
    ) -> Result<ShellHandle, TransportError> {
        // Subprocess transport does not expose structured PTY IO; it takes
        // over the terminal directly (see `run_interactive`). Callers that
        // need a structured PTY must use a transport whose backend supports it.
        Err(TransportError::fallback(anyhow::anyhow!(
            "subprocess transport does not provide structured PTY IO"
        )))
    }

    async fn exec(&self, conn: &Connection, command: &str) -> Result<ExecOutput, TransportError> {
        // Interactive bastions (Kerberos + bastion) cannot pass a remote command in the SSH argv.
        if conn.use_kerberos && conn.bastion.is_some() {
            return self.run_interactive_exec(conn, command).await;
        }

        let argv = Self::build_exec_argv(
            conn,
            command,
            &self.config.transport.strict_host_key_checking,
        );
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;

        let output = prepare_ssh_command(cmd_name, self.config.auth.agent_socket.as_deref())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);

        // Fallback to interactive execution if server complains about missing TTY
        if stdout_str.contains("option -t")
            || stderr_str.contains("option -t")
            || stdout_str.contains("l'option -t")
            || stderr_str.contains("l'option -t")
            || stdout_str.contains("pseudo-terminal")
            || stderr_str.contains("pseudo-terminal")
        {
            return self.run_interactive_exec(conn, command).await;
        }

        Ok(ExecOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn exec_limited(
        &self,
        conn: &Connection,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<LimitedOutput, TransportError> {
        let interactive_bastion = conn.use_kerberos && conn.bastion.is_some();
        if opts.is_plain() {
            let out = self.exec(conn, command).await?;
            return Ok(LimitedOutput::from_output(out, None));
        }
        if interactive_bastion && opts.cert_auth.is_some() {
            return Err(cert_through_kerberos_bastion(conn));
        }
        if opts.non_interactive && opts.cert_auth.is_none() && conn.use_kerberos {
            require_kerberos_ticket(conn).await?;
        }
        if interactive_bastion {
            // With Kerberos but no certificate: PTY scraping works for an agent too,
            // now that the hop is BatchMode and a login prompt fails fast.
            return self.limited_interactive(conn, command, opts).await;
        }

        // Keep the key and certificate on disk only for the life of this call.
        let cert_files = match &opts.cert_auth {
            Some(cert) => Some(write_cert_files(cert).map_err(TransportError::permanent)?),
            None => None,
        };
        if opts.non_interactive && cert_files.is_none() {
            if let Some(dir) = &opts.control_dir {
                prepare_control_dir(dir)?;
            }
        }
        let extras = ArgvExtras {
            non_interactive: opts.non_interactive,
            connect_timeout_secs: opts.connect_timeout.map(|t| t.as_secs().max(1)),
            cert: cert_files
                .as_ref()
                .map(|(_, key, cert)| (key.clone(), cert.clone())),
            control_dir: opts.control_dir.clone(),
        };
        let argv = Self::build_exec_argv_with(
            conn,
            command,
            &self.config.transport.strict_host_key_checking,
            &extras,
        );
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;
        let out = run_argv_limited(
            cmd_name,
            args,
            opts,
            self.config.auth.agent_socket.as_deref(),
        )
        .await?;
        drop(cert_files);

        if opts.non_interactive {
            if let Some(err) = classify_ssh_failure(&out, Some(conn)) {
                return Err(err);
            }
        } else if !out.timed_out && needs_tty(&out.stdout, &out.stderr) {
            // Same fallback as plain `exec`: the server wants a PTY.
            return self.limited_interactive(conn, command, opts).await;
        }
        Ok(out)
    }

    async fn open_sftp(&self, _conn: &Connection) -> Result<Box<dyn SftpSession>, TransportError> {
        Err(TransportError::fallback(anyhow::anyhow!(
            "subprocess transport does not provide structured SFTP"
        )))
    }

    async fn forward_local(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        remote_host: &str,
        remote_port: u16,
    ) -> Result<crate::services::transport::types::ForwardHandle, TransportError> {
        let argv = Self::build_forward_argv(
            conn,
            bind_host,
            bind_port,
            remote_host,
            remote_port,
            &self.config.transport.strict_host_key_checking,
        );
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::Permanent(anyhow::anyhow!("empty argv")))?;

        let mut child = prepare_ssh_command(cmd_name, self.config.auth.agent_socket.as_deref())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| TransportError::Permanent(anyhow::anyhow!("spawn: {e}")))?;

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = cancel_rx => { let _ = child.kill().await; }
                _ = child.wait() => {}
            }
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
        let argv = Self::build_dynamic_argv(
            conn,
            bind_host,
            bind_port,
            &self.config.transport.strict_host_key_checking,
        );
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::Permanent(anyhow::anyhow!("empty argv")))?;

        let mut child = prepare_ssh_command(cmd_name, self.config.auth.agent_socket.as_deref())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| TransportError::Permanent(anyhow::anyhow!("spawn: {e}")))?;

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = cancel_rx => { let _ = child.kill().await; }
                _ = child.wait() => {}
            }
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
        let argv =
            Self::build_shell_argv_with_shkc(conn, self.config.transport.host_key_policy(), true);
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;

        let Some(recorder) = recorder else {
            let mut child = prepare_ssh_command(cmd_name, self.config.auth.agent_socket.as_deref())
                .args(args)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

            let status = child
                .wait()
                .await
                .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

            return Ok(status.code().unwrap_or(-1));
        };

        struct RawModeGuard(bool);
        impl RawModeGuard {
            fn enter() -> Self {
                Self(crossterm::terminal::enable_raw_mode().is_ok())
            }
        }
        impl Drop for RawModeGuard {
            fn drop(&mut self) {
                if self.0 {
                    let _ = crossterm::terminal::disable_raw_mode();
                }
            }
        }

        let (cols, rows) = crossterm::terminal::size().unwrap_or((
            crate::services::pty::DEFAULT_COLS,
            crate::services::pty::DEFAULT_ROWS,
        ));
        let spawned = crate::services::pty::spawn(crate::services::pty::PtySpawnOptions {
            cmd_name,
            args,
            rows,
            cols,
            agent_socket: self.config.auth.agent_socket.as_deref(),
        })
        .map_err(|e| TransportError::permanent(anyhow::anyhow!(e)))?;

        let _raw_guard = RawModeGuard::enter();

        let mut pty_writer = spawned.writer;
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 4096];
            loop {
                match stdin.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if pty_writer.write_all(&buf[..n]).is_err() || pty_writer.flush().is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });

        let mut pty_reader = spawned.reader;
        let rec_for_reader = std::sync::Arc::clone(&recorder);
        let reader_handle = std::thread::spawn(move || {
            use std::io::Write;
            let mut stdout = std::io::stdout();
            crate::services::pty::read_loop(&mut pty_reader, |chunk| {
                let _ = stdout.write_all(chunk.as_bytes());
                let _ = stdout.flush();
                if let Ok(mut g) = rec_for_reader.lock() {
                    g.output(&chunk);
                }
            });
        });

        let master = spawned.master;
        let mut child = spawned.child;
        let mut wait_fut = tokio::task::spawn_blocking(move || child.wait());
        #[cfg(unix)]
        let mut sigwinch =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()).ok();

        let wait_res = loop {
            #[cfg(unix)]
            if let Some(ref mut sig) = sigwinch {
                tokio::select! {
                    res = &mut wait_fut => break res,
                    _ = sig.recv() => {
                        if let Ok((c, r)) = crossterm::terminal::size() {
                            let _ = crate::services::pty::resize(master.as_ref(), c, r);
                            if let Ok(mut g) = recorder.lock() {
                                g.resize(c, r);
                            }
                        }
                    }
                }
            } else {
                break (&mut wait_fut).await;
            }
            #[cfg(not(unix))]
            break (&mut wait_fut).await;
        };

        drop(master);
        let _ = reader_handle.join();
        drop(_raw_guard);

        let status = wait_res
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        Ok(status.exit_code() as i32)
    }

    fn name(&self) -> &'static str {
        "subprocess"
    }
}
