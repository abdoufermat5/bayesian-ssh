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
    /// Pulled out for unit-testing the command assembly.
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

    /// Build the argv for an interactive shell that takes over the terminal.
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
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push(format!("{}@{}", conn.user, conn.host));
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

    fn name(&self) -> &'static str {
        "subprocess"
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
