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

/// POSIX single-quote a value for interpolation into a shell command line
/// (`sh -c`). `ssh -o ProxyCommand=...` runs its value through the shell, so
/// any unquoted user-controlled string (bastion host, key path, …) is a
/// command-injection vector. Single quotes make the value opaque to the
/// shell; embedded `'` is escaped as `'"'"'`.
pub(crate) fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if !value.contains([
        '\'', '\\', ' ', '\t', '\n', '"', '$', '`', ';', '&', '|', '<', '>', '(', ')', '*', '?',
        '[', ']', '{', '}', '!', '#', '~',
    ]) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Quote a value for a `ProxyCommand`: ssh first performs `%` token
/// expansion, then hands the string to `sh -c`, so literal `%` must be
/// doubled on top of the shell quoting.
fn proxy_quote(value: &str) -> String {
    shell_quote(value).replace('%', "%%")
}

/// Map the config policy (`strict` | `accept-new` | `off`) onto a value the
/// OpenSSH client accepts. `strict` is not an OpenSSH keyword — passing it
/// verbatim makes `ssh` abort with "unsupported option".
pub(crate) fn ssh_strict_host_key_value(policy: &str) -> &'static str {
    match policy {
        "strict" | "yes" => "yes",
        "off" | "no" => "no",
        _ => "accept-new",
    }
}

/// Build the `ProxyCommand=` value for a classic jump host.
///
/// `leading_opts` are inserted right after `ssh` (e.g. TTY flags). Every
/// connection field is quoted, the bastion destination follows `--` so a
/// value starting with `-` cannot become an option, and the target is only
/// passed as `%h` when it is shell-inert: OpenSSH < 9.6 substitutes `%h`
/// into the shell command unquoted.
pub(crate) fn jump_proxy_command(conn: &Connection, leading_opts: &str, shkc: &str) -> String {
    let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
    let bastion = conn.bastion.as_deref().unwrap_or_default();
    let key_flag = conn
        .key_path
        .as_deref()
        .map(|k| format!(" -i {}", proxy_quote(k)))
        .unwrap_or_default();
    let target = if shell_quote(&conn.host) == conn.host {
        "%h:%p".to_string()
    } else if conn.host.contains(':') {
        proxy_quote(&format!("[{}]:{}", conn.host, conn.port))
    } else {
        proxy_quote(&format!("{}:{}", conn.host, conn.port))
    };
    format!(
        "ssh{leading_opts}{key_flag} -o StrictHostKeyChecking={} -W {target} -- {}@{}",
        ssh_strict_host_key_value(shkc),
        proxy_quote(bu),
        proxy_quote(bastion)
    )
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
    pub(crate) fn build_exec_argv(conn: &Connection, command: &str, shkc: &str) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        argv.push("-tt".into());
        argv.push("-o".into());
        argv.push("RequestTTY=force".into());
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));

        if conn.bastion.is_some() {
            argv.push("-o".into());
            argv.push(format!(
                "ProxyCommand={}",
                jump_proxy_command(conn, " -tt -o RequestTTY=force", shkc)
            ));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        // `--` so a user/host beginning with `-` cannot be parsed as an option.
        argv.push("--".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv.push(command.to_string());
        argv
    }

    /// Build the argv for an interactive shell session.
    ///
    /// When Kerberos + bastion are both active the bastion is an *interactive*
    /// bastion: we SSH into it and pass `target_user@target` as argument.
    /// Without Kerberos the bastion is a classic jump host using ProxyCommand with forced TTY.
    ///
    /// The target's host-key policy is left to the user's ssh configuration
    /// (OpenSSH default: interactive `ask` prompt in the terminal).
    pub fn build_shell_argv(conn: &Connection) -> Vec<String> {
        Self::build_shell_argv_with_shkc(conn, "accept-new", false)
    }

    /// `enforce_target_shkc`: also pass the policy for the target connection
    /// (CLI paths honour `transport.strict_host_key_checking`).
    fn build_shell_argv_with_shkc(
        conn: &Connection,
        shkc: &str,
        enforce_target_shkc: bool,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        // Force remote TTY allocation even when stdin is not a terminal (e.g. GUI background process)
        argv.push("-tt".into());
        argv.push("-o".into());
        argv.push("RequestTTY=force".into());
        if conn.use_kerberos {
            argv.push("-A".into());
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        if enforce_target_shkc {
            argv.push("-o".into());
            argv.push(format!(
                "StrictHostKeyChecking={}",
                ssh_strict_host_key_value(shkc)
            ));
        }

        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            if conn.use_kerberos {
                // Interactive bastion: connect to bastion, pass target as argument.
                argv.push("-p".into());
                argv.push("22".into());
                argv.push("--".into());
                argv.push(format!("{bu}@{bastion}"));
                argv.push(format!("{}@{}", conn.user, conn.host));
            } else {
                // ProxyCommand with forced TTY allocation on the bastion connection
                argv.push("-o".into());
                argv.push(format!(
                    "ProxyCommand={}",
                    jump_proxy_command(conn, " -tt -o RequestTTY=force", shkc)
                ));
                argv.push("-p".into());
                argv.push(conn.port.to_string());
                argv.push("--".into());
                argv.push(format!("{}@{}", conn.user, conn.host));
            }
        } else {
            argv.push("-p".into());
            argv.push(conn.port.to_string());
            argv.push("--".into());
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
        shkc: &str,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));
        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push("-L".into());
        argv.push(format!(
            "{bind_host}:{bind_port}:{remote_host}:{remote_port}"
        ));
        argv.push("-N".into());
        argv.push("--".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv
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

        let mut argv = Self::build_shell_argv_with_shkc(
            conn,
            &self.config.transport.strict_host_key_checking,
            true,
        );
        if let Some(pos) = argv.iter().position(|a| a == "-t" || a == "-tt") {
            argv[pos] = "-tt".into();
        }
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;

        let mut child = TokioCommand::new(cmd_name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::permanent(anyhow::Error::from(e)))?;

        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                let _ = child.kill().await;
                return Err(TransportError::permanent(anyhow::anyhow!(
                    "stdin pipe not available"
                )));
            }
        };
        let mut stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                let _ = child.kill().await;
                return Err(TransportError::permanent(anyhow::anyhow!(
                    "stdout pipe not available"
                )));
            }
        };
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
        // Widen the PTY so `ls` and other column-aware tools don't
        // wrap/pad to the default 80-col width.
        let payload = format!(
            "stty cols 200 rows 50 2>/dev/null\necho '{marker_start}'\n{command}\n_bssh_rc=$?\necho '{marker_end}'\nexit $_bssh_rc\n"
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
            command,                 // the real command echoed
            "_bssh_rc=$?",           // exit-code capture
            "exit $_bssh_rc",        // exit command
            "stty cols",             // PTY resize command
            echo_start_cmd.as_str(), // echo of START marker
            echo_end_cmd.as_str(),   // echo of END marker
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
                let has_prompt =
                    trimmed.contains("% ") || trimmed.contains("$ ") || trimmed.contains("# ");
                let is_echo = has_prompt && echo_suffixes.iter().any(|s| trimmed.contains(s));
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

        let output = TokioCommand::new(cmd_name)
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

        Ok(crate::services::transport::types::ForwardHandle::new(
            task, cancel_tx,
        ))
    }

    async fn run_interactive(&self, conn: &Connection) -> Result<i32, TransportError> {
        let argv = Self::build_shell_argv_with_shkc(
            conn,
            &self.config.transport.strict_host_key_checking,
            true,
        );
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;

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
        shkc: &str,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));
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
        argv.push("--".into());
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
        let argv =
            SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "accept-new");
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
        let argv =
            SubprocessTransport::build_exec_argv(&c(true, None, None), "uptime", "accept-new");
        assert!(argv.contains(&"-K".to_string()));
    }

    #[test]
    fn argv_bastion_uses_proxy_command() {
        let argv = SubprocessTransport::build_exec_argv(
            &c(false, Some("b.example"), None),
            "uptime",
            "accept-new",
        );
        assert!(argv.iter().any(|a| a.contains("ProxyCommand=")));
        assert!(argv.iter().any(|a| a.contains("alice@b.example")));
    }

    #[test]
    fn argv_kerberos_bastion_uses_proxy_command() {
        let argv = SubprocessTransport::build_exec_argv(
            &c(true, Some("b.example"), None),
            "ls -l /tmp",
            "accept-new",
        );
        assert!(argv.iter().any(|a| a.contains("ProxyCommand=")));
        assert!(argv.contains(&"-K".to_string()));
        assert!(argv.iter().any(|a| a.contains("alice@b.example")));
        assert!(argv.contains(&"ls -l /tmp".to_string()));
    }

    #[test]
    fn argv_key_path_uses_i_flag() {
        let argv = SubprocessTransport::build_exec_argv(
            &c(false, None, Some("/k/id_ed25519")),
            "uptime",
            "accept-new",
        );
        assert!(argv.contains(&"-i".to_string()));
        assert!(argv.contains(&"/k/id_ed25519".to_string()));
    }

    #[test]
    fn argv_shkc_threads_config_value() {
        let argv = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "strict");
        assert!(argv.iter().any(|a| a == "StrictHostKeyChecking=yes"));
        let argv_off = SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "off");
        assert!(argv_off.iter().any(|a| a == "StrictHostKeyChecking=no"));
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
        assert!(argv.contains(&"-tt".to_string()));
        assert!(argv.contains(&"-K".to_string()));
    }

    #[test]
    fn destination_follows_double_dash() {
        let mut conn = c(false, None, None);
        conn.user = "-oProxyCommand=touch /tmp/pwned".into();
        let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
        let dd = argv.iter().position(|a| a == "--").expect("-- present");
        assert!(argv[dd + 1].starts_with("-oProxyCommand"));
        let shell = SubprocessTransport::build_shell_argv(&conn);
        let dd = shell.iter().position(|a| a == "--").expect("-- present");
        assert_eq!(dd, shell.len() - 2);
    }

    #[test]
    fn proxy_command_never_expands_unsafe_host() {
        let mut conn = c(false, Some("b.example"), None);
        conn.host = "x$(touch /tmp/pwned)".into();
        let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
        let proxy = argv
            .iter()
            .find(|a| a.starts_with("ProxyCommand="))
            .unwrap();
        assert!(!proxy.contains("%h"));
        assert!(proxy.contains("'x$(touch /tmp/pwned):2222'"));
        assert!(proxy.ends_with("-- alice@b.example"));

        // Shell-inert hosts keep `%h` so ~/.ssh/config HostName aliases work.
        let argv = SubprocessTransport::build_exec_argv(
            &c(false, Some("b.example"), None),
            "uptime",
            "accept-new",
        );
        let proxy = argv
            .iter()
            .find(|a| a.starts_with("ProxyCommand="))
            .unwrap();
        assert!(proxy.contains("-W %h:%p"));
    }

    #[test]
    fn proxy_command_escapes_percent() {
        let mut conn = c(false, Some("b.example"), Some("/keys/100%key"));
        conn.bastion_user = Some("ops".into());
        let proxy = jump_proxy_command(&conn, "", "accept-new");
        assert!(proxy.contains("-i /keys/100%%key"));
    }
}
