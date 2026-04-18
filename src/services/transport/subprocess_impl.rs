//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

#![allow(dead_code)]

use crate::config::AppConfig;
use crate::models::Connection;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

use super::types::{ExecOutput, PtyIo, SftpSession, ShellHandle, SshTransport, TransportError};

pub struct SubprocessTransport {
    #[allow(dead_code)]
    config: AppConfig,
}

impl SubprocessTransport {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    /// Build the argv for a non-interactive exec call.
    ///
    /// NOTE: this is used for direct connections and classic jump-host
    /// bastions only.  Interactive bastions (kerberos + bastion) cannot
    /// pass a remote command via the argv — use `run_interactive_exec`
    /// instead.
    pub(crate) fn build_exec_argv(conn: &Connection, command: &str) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push("BatchMode=yes".into());
        argv.push("-o".into());
        argv.push("StrictHostKeyChecking=accept-new".into());

        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv.push(command.to_string());
        argv
    }

    /// Build the argv for an interactive shell session.
    ///
    /// When Kerberos + bastion are both active the bastion is an *interactive*
    /// bastion: we SSH into it and pass `target_user@target` as argument.
    /// Without Kerberos the bastion is a classic jump host and `-J` is used.
    pub(crate) fn build_shell_argv(conn: &Connection) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-t".into());
            argv.push("-A".into());
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }

        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            if conn.use_kerberos {
                // Interactive bastion: connect to bastion, pass target as argument.
                argv.push("-p".into());
                argv.push("22".into());
                argv.push(format!("{bu}@{bastion}"));
                argv.push(format!("{}@{}", conn.user, conn.host));
            } else {
                // Jump host: transparent forwarding via -J.
                argv.push("-J".into());
                argv.push(format!("{bu}@{bastion}"));
                argv.push("-p".into());
                argv.push(conn.port.to_string());
                argv.push(format!("{}@{}", conn.user, conn.host));
            }
        } else {
            argv.push("-p".into());
            argv.push(conn.port.to_string());
            argv.push(format!("{}@{}", conn.user, conn.host));
        }
        argv
    }

    /// Build the argv for a local port-forward session (`ssh -L -N`).
    pub(crate) fn build_forward_argv(
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        remote_host: &str,
        remote_port: u16,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push("-L".into());
        argv.push(format!("{bind_host}:{bind_port}:{remote_host}:{remote_port}"));
        argv.push("-N".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv
    }

    /// Run an interactive SSH session that takes over the current terminal.
    /// Returns the exit code of the ssh process.
    pub async fn run_interactive(&self, conn: &Connection) -> Result<i32, TransportError> {
        let argv = Self::build_shell_argv(conn);
        let (cmd_name, args) = argv.split_first().expect("argv non-empty");

        let mut child = TokioCommand::new(cmd_name)
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

        Ok(status.code().unwrap_or(-1))
    }

    /// Execute a command through an interactive bastion.
    ///
    /// Interactive bastions only accept the target as an argument — they
    /// do NOT forward extra arguments as a remote command.  We open a
    /// shell-style connection in three phases:
    ///
    /// 1. **Drain** — read and discard the initial noise (bastion banner,
    ///    MOTD, shell prompt) until 1 s of silence.
    /// 2. **Payload** — send markers + command.
    /// 3. **Extract** — read remaining output, extract lines between markers,
    ///    and strip echoed control commands via suffix matching.
    pub async fn run_interactive_exec(
        &self,
        conn: &Connection,
        command: &str,
    ) -> Result<ExecOutput, TransportError> {
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let marker = format!("BSSH_{:016x}", rand::random::<u64>());
        let marker_start = format!("{marker}_START");
        let marker_end = format!("{marker}_END");

        let mut argv = Self::build_shell_argv(conn);
        if let Some(pos) = argv.iter().position(|a| a == "-t") {
            argv[pos] = "-tt".into();
        }
        let (cmd_name, args) = argv.split_first().expect("argv non-empty");

        let mut child = TokioCommand::new(cmd_name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        let mut stdin = child.stdin.take().expect("stdin piped");
        let mut stdout = child.stdout.take().expect("stdout piped");
        // With -tt stderr is merged into stdout via the PTY.
        let _stderr = child.stderr.take();

        // ── Phase 1: drain initial noise (banner, MOTD, prompt) ──
        loop {
            let mut chunk = [0u8; 4096];
            match tokio::time::timeout(Duration::from_secs(1), stdout.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => { /* discard */ }
                _ => break,
            }
        }

        // ── Phase 2: send markers + command ──
        let payload = format!(
            "echo '{marker_start}'\n{command}\n_bssh_rc=$?\necho '{marker_end}'\nexit $_bssh_rc\n"
        );
        let _ = stdin.write_all(payload.as_bytes()).await;
        drop(stdin);

        // ── Phase 3: read remaining output and extract between markers ──
        let mut raw_bytes = Vec::new();
        stdout
            .read_to_end(&mut raw_bytes)
            .await
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        let status = child
            .wait()
            .await
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        // Build the set of suffixes that correspond to echoed control
        // commands.  The PTY echoes them with a prompt prefix we can't
        // predict, but the line always contains our known payload text.
        let echo_start_cmd = format!("echo '{marker_start}'");
        let echo_end_cmd = format!("echo '{marker_end}'");
        let echo_suffixes: Vec<&str> = vec![
            command,                  // the real command echoed
            "_bssh_rc=$?",            // exit-code capture
            "exit $_bssh_rc",         // exit command
            echo_start_cmd.as_str(),  // echo of START marker
            echo_end_cmd.as_str(),    // echo of END marker
        ];

        let raw = String::from_utf8_lossy(&raw_bytes);
        let mut capture = false;
        let mut clean_lines: Vec<&str> = Vec::new();

        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed == marker_start || trimmed.ends_with(&marker_start) {
                capture = true;
                continue;
            }
            if trimmed == marker_end || trimmed.ends_with(&marker_end) {
                capture = false;
                continue;
            }
            if capture {
                // Skip lines that are the PTY echoing our control commands.
                // Echo lines from the bastion shell always carry a prompt
                // indicator (`% ` for zsh, `$ ` for bash, `# ` for root).
                let has_prompt = trimmed.contains("% ")
                    || trimmed.contains("$ ")
                    || trimmed.contains("# ");
                let is_echo = has_prompt
                    && echo_suffixes.iter().any(|s| trimmed.contains(s));
                if !is_echo {
                    clean_lines.push(line.trim_end_matches('\r'));
                }
            }
        }

        let stdout_bytes = if clean_lines.is_empty() {
            raw_bytes
        } else {
            let mut joined = clean_lines.join("\n");
            joined.push('\n');
            joined.into_bytes()
        };

        Ok(ExecOutput {
            stdout: stdout_bytes,
            stderr: Vec::new(),
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

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

    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
    ) -> Result<ExecOutput, TransportError> {
        // Interactive bastions cannot pass a remote command in the SSH argv.
        // Delegate to the interactive exec path which pipes it via stdin.
        if conn.use_kerberos && conn.bastion.is_some() {
            return self.run_interactive_exec(conn, command).await;
        }

        let argv = Self::build_exec_argv(conn, command);
        let (cmd_name, args) = argv.split_first().expect("argv non-empty");

        let output = TokioCommand::new(cmd_name)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        Ok(ExecOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn open_sftp(
        &self,
        _conn: &Connection,
    ) -> Result<Box<dyn SftpSession>, TransportError> {
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
        let argv = Self::build_forward_argv(conn, bind_host, bind_port, remote_host, remote_port);
        let (cmd_name, args) = argv.split_first().expect("argv non-empty");

        let mut child = TokioCommand::new(cmd_name)
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

        Ok(crate::services::transport::types::ForwardHandle::new(task, cancel_tx))
    }

    async fn forward_dynamic(
        &self,
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<crate::services::transport::types::ForwardHandle, TransportError> {
        let argv = Self::build_dynamic_argv(conn, bind_host, bind_port);
        let (cmd_name, args) = argv.split_first().expect("argv non-empty");

        let mut child = TokioCommand::new(cmd_name)
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

        Ok(crate::services::transport::types::ForwardHandle::new(task, cancel_tx))
    }

    fn name(&self) -> &'static str {
        "subprocess"
    }
}

impl SubprocessTransport {
    /// Build the argv for a SOCKS5 dynamic proxy session (`ssh -D -N`).
    pub(crate) fn build_dynamic_argv(
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push("-D".into());
        // Use bind_host:port form only when bind_host is not loopback.
        if bind_host == "127.0.0.1" || bind_host == "localhost" {
            argv.push(format!("{bind_port}"));
        } else {
            argv.push(format!("{bind_host}:{bind_port}"));
        }
        argv.push("-N".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(kerb: bool, bastion: Option<&str>, key: Option<&str>) -> Connection {
        Connection::new(
            "x".into(),
            "target.example".into(),
            "alice".into(),
            2222,
            bastion.map(String::from),
            None,
            kerb,
            key.map(String::from),
        )
    }

    #[test]
    fn argv_simple() {
        let argv = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime");
        assert_eq!(argv[0], "ssh");
        assert!(argv.contains(&"-p".to_string()));
        assert!(argv.contains(&"2222".to_string()));
        assert!(argv.contains(&"alice@target.example".to_string()));
        assert_eq!(argv.last().unwrap(), "uptime");
        // no -K, no -i, no -J
        assert!(!argv.contains(&"-K".to_string()));
        assert!(!argv.contains(&"-i".to_string()));
        assert!(!argv.contains(&"-J".to_string()));
    }

    #[test]
    fn argv_kerberos_adds_k_flag() {
        let argv = SubprocessTransport::build_exec_argv(&c(true, None, None), "uptime");
        assert!(argv.contains(&"-K".to_string()));
    }

    #[test]
    fn argv_bastion_uses_j_flag() {
        let argv =
            SubprocessTransport::build_exec_argv(&c(false, Some("b.example"), None), "uptime");
        assert!(argv.contains(&"-J".to_string()));
        assert!(argv.contains(&"alice@b.example".to_string()));
    }

    #[test]
    fn argv_kerberos_bastion_still_uses_j_flag() {
        // build_exec_argv is only used for non-interactive-bastion paths,
        // but verify it produces valid argv even when called with kerberos+bastion.
        // At runtime, the exec trait method short-circuits to run_interactive_exec instead.
        let argv =
            SubprocessTransport::build_exec_argv(&c(true, Some("b.example"), None), "ls -l /tmp");
        assert!(argv.contains(&"-J".to_string()));
        assert!(argv.contains(&"-K".to_string()));
        assert!(argv.contains(&"alice@b.example".to_string()));
        assert!(argv.contains(&"ls -l /tmp".to_string()));
    }

    #[test]
    fn argv_key_path_uses_i_flag() {
        let argv = SubprocessTransport::build_exec_argv(
            &c(false, None, Some("/k/id_ed25519")),
            "uptime",
        );
        assert!(argv.contains(&"-i".to_string()));
        assert!(argv.contains(&"/k/id_ed25519".to_string()));
    }

    #[test]
    fn shell_argv_simple() {
        let argv = SubprocessTransport::build_shell_argv(&c(false, None, None));
        assert_eq!(argv[0], "ssh");
        assert!(argv.contains(&"-p".to_string()));
        assert!(argv.contains(&"2222".to_string()));
        assert!(argv.last().unwrap().contains("alice@target.example"));
    }

    #[test]
    fn shell_argv_kerberos_adds_flags() {
        let argv = SubprocessTransport::build_shell_argv(&c(true, None, None));
        assert!(argv.contains(&"-t".to_string()));
        assert!(argv.contains(&"-K".to_string()));
    }
}
