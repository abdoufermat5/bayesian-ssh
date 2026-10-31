//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::config::{AppConfig, HostKeyPolicy};
use crate::models::Connection;
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command as TokioCommand;

use super::types::{
    CappedBuf, CertAuth, ExecOptions, ExecOutput, LimitedOutput, PtyIo, SftpSession, ShellHandle,
    SshTransport, TransportError,
};

pub struct SubprocessTransport {
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

/// Agent and CI shells often export `TERM=dumb`. Some Kerberos bastions then
/// refuse the session with a misleading "add -t" message even when `-tt` was
/// sent. Give `ssh` a real terminal type whenever ours is missing or dumb.
fn prepare_ssh_command(program: &str) -> TokioCommand {
    let mut cmd = TokioCommand::new(program);
    let bad_term = match std::env::var_os("TERM") {
        None => true,
        Some(t) if t.is_empty() => true,
        Some(t) => t.eq_ignore_ascii_case("dumb"),
    };
    if bad_term {
        cmd.env("TERM", "xterm-256color");
    }
    cmd
}

/// Map the config policy (`strict` | `accept-new` | `off`, plus the OpenSSH
/// `yes`/`no` aliases) onto a value the OpenSSH client accepts. `strict` is
/// not an OpenSSH keyword — passing it verbatim makes `ssh` abort with
/// "unsupported option". An unrecognised value fails closed to `yes`.
pub(crate) fn ssh_strict_host_key_value(policy: &str) -> &'static str {
    match HostKeyPolicy::from_config(policy) {
        HostKeyPolicy::Strict => "yes",
        HostKeyPolicy::AcceptNew => "accept-new",
        HostKeyPolicy::Off => "no",
    }
}

/// Extra `ssh` switches for the jump-host hop (`ProxyCommand`).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HopOptions {
    /// Never prompt on the hop: a broken bastion login fails instead of waiting for input.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
}

/// Build the `ProxyCommand=` value for a classic jump host.
///
/// `leading_opts` are inserted right after `ssh` (e.g. TTY flags). `hop`
/// carries the non-interactive switches. Every connection field is quoted, the
/// bastion destination follows `--` so a value starting with `-` cannot become
/// an option, and the target is only passed as `%h` when it is shell-inert:
/// OpenSSH < 9.6 substitutes `%h` into the shell command unquoted.
pub(crate) fn jump_proxy_command(
    conn: &Connection,
    leading_opts: &str,
    shkc: &str,
    hop: &HopOptions,
) -> String {
    let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
    let bastion = conn.bastion.as_deref().unwrap_or_default();
    let key_flag = conn
        .key_path
        .as_deref()
        .map(|k| format!(" -i {}", proxy_quote(k)))
        .unwrap_or_default();
    let mut hop_flags = String::new();
    if hop.non_interactive {
        hop_flags.push_str(" -o BatchMode=yes");
    }
    if let Some(secs) = hop.connect_timeout_secs {
        hop_flags.push_str(&format!(" -o ConnectTimeout={secs}"));
    }
    let target = if shell_quote(&conn.host) == conn.host {
        "%h:%p".to_string()
    } else if conn.host.contains(':') {
        proxy_quote(&format!("[{}]:{}", conn.host, conn.port))
    } else {
        proxy_quote(&format!("{}:{}", conn.host, conn.port))
    };
    format!(
        "ssh{leading_opts}{hop_flags}{key_flag} -o StrictHostKeyChecking={} -W {target} -- {}@{}",
        ssh_strict_host_key_value(shkc),
        proxy_quote(bu),
        proxy_quote(bastion)
    )
}

/// Switches for a shell-style (PTY) connection driven by an agent run.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ShellExtras {
    /// Never prompt on any hop; no agent or port forwarding.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
}

/// Extra `ssh` switches for [`SshTransport::exec_limited`].
#[derive(Debug, Default, Clone)]
pub(crate) struct ArgvExtras {
    /// `BatchMode=yes`, no agent/X11/port forwarding.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
    /// Private key and certificate files to authenticate with (and nothing else).
    pub cert: Option<(PathBuf, PathBuf)>,
    /// Directory for `ControlMaster` sockets (run-scoped SSH connection sharing).
    pub control_dir: Option<PathBuf>,
}

/// Stderr fragments that mean `ssh` itself failed (exit 255), not the remote command.
const SSH_FAILURE_MARKERS: &[&str] = &[
    "Permission denied",
    "Host key verification failed",
    "REMOTE HOST IDENTIFICATION HAS CHANGED",
    "Connection refused",
    "Connection timed out",
    "Operation timed out",
    "Could not resolve hostname",
    "No route to host",
    "Connection closed by",
    "Connection reset by",
    "kex_exchange_identification",
    "ssh_exchange_identification",
    "administratively prohibited",
];

/// In batch mode, turn an `ssh` failure (exit 255 with a known message) into an error that
/// says what to do, instead of a result an agent might take for the command's own output.
///
/// `conn` names the route, so a refused login is attributed to the hop that refused it
/// (the bastion or the target) and a Kerberos host gets the ticket hint.
fn classify_ssh_failure(out: &LimitedOutput, conn: Option<&Connection>) -> Option<TransportError> {
    if out.timed_out || out.exit_code != Some(255) {
        return None;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("administratively prohibited") {
        let (bastion, name) = match conn {
            Some(c) => (
                c.bastion.as_deref().unwrap_or("the bastion").to_string(),
                c.name.clone(),
            ),
            None => ("the bastion".to_string(), "this host".to_string()),
        };
        return Some(TransportError::permanent(anyhow::anyhow!(
            "{bastion} forbids TCP forwarding, which a jump host needs; \
             if {name} is behind an interactive (Kerberos) bastion, say so in bssh \
             (bssh edit {name} --kerberos true)"
        )));
    }
    let marker = SSH_FAILURE_MARKERS.iter().find(|m| stderr.contains(**m))?;
    let line = stderr
        .lines()
        .find(|l| l.contains(*marker))
        .unwrap_or_default()
        .trim();

    // A Kerberos rejection is about the ticket, not about keys: say which one to fix.
    if conn.is_some_and(|c| c.use_kerberos) && stderr.to_ascii_lowercase().contains("gssapi") {
        return Some(TransportError::permanent(anyhow::anyhow!(
            "Kerberos login failed ({line}); check for a ticket with `klist` and get one with `kinit`"
        )));
    }

    if *marker == "Permission denied" {
        // The bastion hop prints `user@bastion: Permission denied (...)`; name that hop,
        // otherwise the agent reads it as the target refusing a key that works on it.
        if let Some(conn) = conn {
            if let Some(bastion) = &conn.bastion {
                let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
                if line.starts_with(&format!("{bu}@")) {
                    return Some(TransportError::permanent(anyhow::anyhow!(
                        "the bastion {bastion} refused the login ({line}); \
                         use a key or ssh-agent that works on the bastion"
                    )));
                }
            }
        }
        return Some(TransportError::permanent(anyhow::anyhow!(
            "needs interactive auth; use a key or ssh-agent ({line})"
        )));
    }
    Some(TransportError::permanent(anyhow::anyhow!("ssh: {line}")))
}

/// Read a pipe to EOF, keeping what `buf` allows and dropping the rest.
async fn drain_capped<R: AsyncRead + Unpin>(reader: Option<R>, buf: &mut CappedBuf) {
    let Some(mut reader) = reader else { return };
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.push(&chunk[..n]),
        }
    }
}

/// Run `program args` with the limits in `opts`. The timeout covers the whole process
/// lifetime (connect included); on expiry the child is killed and the partial output kept.
/// Split from `exec_limited` so tests can drive it with `sh -c`.
pub(crate) async fn run_argv_limited(
    program: &str,
    args: &[String],
    opts: &ExecOptions,
) -> Result<LimitedOutput, TransportError> {
    let mut child = prepare_ssh_command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| TransportError::permanent(anyhow::anyhow!("spawn {program}: {e}")))?;

    let mut stdout = CappedBuf::new(opts.max_output_bytes);
    let mut stderr = CappedBuf::new(opts.max_output_bytes);
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let run = async {
        tokio::join!(
            drain_capped(stdout_pipe, &mut stdout),
            drain_capped(stderr_pipe, &mut stderr)
        );
        child.wait().await
    };
    let status = match opts.timeout {
        Some(t) => tokio::time::timeout(t, run).await.ok(),
        None => Some(run.await),
    };
    let (exit_code, timed_out) = match status {
        Some(Ok(st)) => (Some(st.code().unwrap_or(-1)), false),
        Some(Err(e)) => return Err(TransportError::permanent(anyhow::Error::from(e))),
        None => {
            let _ = child.kill().await;
            (None, true)
        }
    };
    Ok(LimitedOutput {
        truncated: stdout.truncated() || stderr.truncated(),
        stdout: stdout.into_vec(),
        stderr: stderr.into_vec(),
        exit_code,
        timed_out,
    })
}

/// Write the certificate identity into a private temp dir (0700, key 0600) that is
/// removed when the returned guard drops.
fn write_cert_files(cert: &CertAuth) -> anyhow::Result<(tempfile::TempDir, PathBuf, PathBuf)> {
    use std::io::Write;

    let dir = tempfile::Builder::new().prefix("bssh-cert-").tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let key_path = dir.path().join("id");
    let cert_path = dir.path().join("id-cert.pub");

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut key_file = opts.open(&key_path)?;
    key_file.write_all(cert.private_key_openssh.as_bytes())?;
    if !cert.private_key_openssh.ends_with('\n') {
        key_file.write_all(b"\n")?;
    }
    std::fs::write(&cert_path, &cert.certificate_openssh)?;
    Ok((dir, key_path, cert_path))
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
        Self::build_exec_argv_with(conn, command, shkc, &ArgvExtras::default())
    }

    /// [`Self::build_exec_argv`] plus the AI/limited-exec switches: `BatchMode`,
    /// `ConnectTimeout`, no forwarding, and a certificate identity.
    pub(crate) fn build_exec_argv_with(
        conn: &Connection,
        command: &str,
        shkc: &str,
        extras: &ArgvExtras,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        // Non-interactive: no remote PTY. A forced TTY makes ssh print
        // "Connection to <host> closed." and merges remote stderr into stdout.
        argv.push("-T".into());
        if extras.non_interactive {
            // An agent run must not hand the user's agent or ports to the host.
            argv.push("-a".into());
            argv.push("-x".into());
            argv.push("-o".into());
            argv.push("ClearAllForwardings=yes".into());
            // Never prompt for a password, passphrase or host key.
            argv.push("-o".into());
            argv.push("BatchMode=yes".into());
        }
        if let Some(secs) = extras.connect_timeout_secs {
            argv.push("-o".into());
            argv.push(format!("ConnectTimeout={secs}"));
        }
        if conn.use_kerberos {
            if extras.non_interactive {
                // A ticket may prove who we are; it must not be forwarded to the host.
                argv.push("-o".into());
                argv.push("GSSAPIAuthentication=yes".into());
                argv.push("-o".into());
                argv.push("GSSAPIDelegateCredentials=no".into());
            } else {
                argv.push("-K".into());
            }
        }
        if let Some((key, cert)) = &extras.cert {
            // Only the per-command certificate: no other identity may be offered.
            argv.push("-o".into());
            argv.push("IdentitiesOnly=yes".into());
            argv.push("-i".into());
            argv.push(key.to_string_lossy().into_owned());
            argv.push("-o".into());
            argv.push(format!("CertificateFile={}", cert.to_string_lossy()));
        } else if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        // One ssh connection per run: a bastion handshake is paid once, not per call.
        // Not with a certificate: every certified call is a different identity.
        if extras.non_interactive && extras.cert.is_none() {
            if let Some(dir) = &extras.control_dir {
                argv.push("-o".into());
                argv.push("ControlMaster=auto".into());
                argv.push("-o".into());
                argv.push(format!("ControlPath={}", dir.join("%C").to_string_lossy()));
                argv.push("-o".into());
                argv.push("ControlPersist=60".into());
            }
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));

        if conn.bastion.is_some() {
            let hop = HopOptions {
                non_interactive: extras.non_interactive,
                connect_timeout_secs: extras.connect_timeout_secs,
            };
            argv.push("-o".into());
            argv.push(format!(
                "ProxyCommand={}",
                jump_proxy_command(conn, " -T", shkc, &hop)
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
    /// Without Kerberos the bastion is a classic jump host using ProxyCommand
    /// with forced TTY.
    ///
    /// `policy` is applied to the bastion hop. The target's host-key prompt is
    /// left to the user's ssh configuration (OpenSSH default: interactive
    /// `ask` prompt in the terminal); CLI paths that must enforce the policy
    /// on the target call `build_shell_argv_with_shkc` with enforcement on.
    pub fn build_shell_argv(conn: &Connection, policy: HostKeyPolicy) -> Vec<String> {
        Self::build_shell_argv_with_shkc(conn, policy, false)
    }

    /// `enforce_target_shkc`: also pass the policy for the target connection
    /// (CLI paths honour `transport.strict_host_key_checking`).
    fn build_shell_argv_with_shkc(
        conn: &Connection,
        policy: HostKeyPolicy,
        enforce_target_shkc: bool,
    ) -> Vec<String> {
        Self::build_shell_argv_ext(conn, policy, enforce_target_shkc, &ShellExtras::default())
    }

    /// Shell-style argv with agent switches: `extras.non_interactive` keeps the PTY
    /// (an interactive bastion needs one) but never prompts on any hop, never forwards
    /// the agent, and does not delegate the Kerberos ticket.
    fn build_shell_argv_ext(
        conn: &Connection,
        policy: HostKeyPolicy,
        enforce_target_shkc: bool,
        extras: &ShellExtras,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        // Force remote TTY allocation even when stdin is not a terminal (e.g. GUI background process)
        argv.push("-tt".into());
        argv.push("-o".into());
        argv.push("RequestTTY=force".into());
        // Kerberos + bastion is an interactive bastion: it SSHs to the target as
        // the user, so the agent hop must carry the same credentials a human
        // session uses (`-A` when opted in, TGT delegation for GSSAPI targets).
        let interactive_bastion = conn.use_kerberos && conn.bastion.is_some();
        if extras.non_interactive {
            if interactive_bastion && conn.forward_agent {
                argv.push("-A".into());
            } else {
                // Direct/jump-host agent runs must not hand the user's agent to the host.
                argv.push("-a".into());
                argv.push("-o".into());
                argv.push("ClearAllForwardings=yes".into());
            }
            argv.push("-x".into());
            // Fail instead of waiting for a password/OTP prompt.
            argv.push("-o".into());
            argv.push("BatchMode=yes".into());
            if let Some(secs) = extras.connect_timeout_secs {
                argv.push("-o".into());
                argv.push(format!("ConnectTimeout={secs}"));
            }
        } else if conn.forward_agent {
            // `-A` hands the local agent to the remote host: only when this
            // connection opted in. `-K` (Kerberos TGT delegation) follows
            // `use_kerberos` on its own.
            argv.push("-A".into());
        }
        if conn.use_kerberos {
            if extras.non_interactive {
                argv.push("-o".into());
                argv.push("GSSAPIAuthentication=yes".into());
                argv.push("-o".into());
                // Through an interactive bastion the *target* authenticates with
                // the delegated TGT; without it the bastion gets "Permission denied
                // (gssapi-with-mic)". Direct Kerberos logins must not delegate.
                if interactive_bastion {
                    argv.push("GSSAPIDelegateCredentials=yes".into());
                } else {
                    argv.push("GSSAPIDelegateCredentials=no".into());
                }
            } else {
                argv.push("-K".into());
            }
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        if enforce_target_shkc {
            argv.push("-o".into());
            argv.push(format!(
                "StrictHostKeyChecking={}",
                ssh_strict_host_key_value(policy.as_str())
            ));
        }

        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            if conn.use_kerberos {
                // Interactive bastion: connect to bastion, pass target as argument.
                let (bastion_host, bastion_port) = crate::models::split_bastion(bastion);
                if let Some(port) = bastion_port {
                    argv.push("-p".into());
                    argv.push(port.to_string());
                }
                argv.push("--".into());
                argv.push(format!("{bu}@{bastion_host}"));
                argv.push(format!("{}@{}", conn.user, conn.host));
            } else {
                // ProxyCommand with forced TTY allocation on the bastion connection
                let hop = HopOptions {
                    non_interactive: extras.non_interactive,
                    connect_timeout_secs: extras.connect_timeout_secs,
                };
                argv.push("-o".into());
                argv.push(format!(
                    "ProxyCommand={}",
                    jump_proxy_command(conn, " -tt -o RequestTTY=force", policy.as_str(), &hop)
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
    ///
    /// With `extras.non_interactive` the child carries `BatchMode` and a connect
    /// timeout, and an authentication prompt in the drain phase kills it instead of
    /// waiting for a human that is not there.
    pub async fn run_interactive_exec(
        &self,
        conn: &Connection,
        command: &str,
    ) -> Result<ExecOutput, TransportError> {
        self.run_interactive_exec_ext(conn, command, &ShellExtras::default())
            .await
    }

    async fn run_interactive_exec_ext(
        &self,
        conn: &Connection,
        command: &str,
        extras: &ShellExtras,
    ) -> Result<ExecOutput, TransportError> {
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let marker = format!("BSSH_{:016x}", rand::random::<u64>());
        let marker_start = format!("{marker}_START");
        let marker_end = format!("{marker}_END");

        let mut argv =
            Self::build_shell_argv_ext(conn, self.config.transport.host_key_policy(), true, extras);
        if let Some(pos) = argv.iter().position(|a| a == "-t" || a == "-tt") {
            argv[pos] = "-tt".into();
        }
        let (cmd_name, args) = argv
            .split_first()
            .ok_or_else(|| TransportError::permanent(anyhow::anyhow!("empty argv")))?;

        let mut child = prepare_ssh_command(cmd_name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The overall timeout drops this future: the ssh child must die with it.
            .kill_on_drop(true)
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
        // With -tt the remote command's stderr is merged into stdout via the PTY.
        // ssh's *own* errors stay on its stderr pipe; keep them, but only for batch
        // mode (the failure classifier needs them) and without letting them grow.
        let stderr_task = if extras.non_interactive {
            child.stderr.take().map(|mut pipe| {
                tokio::spawn(async move {
                    let mut buf: Vec<u8> = Vec::new();
                    let mut chunk = [0u8; 2048];
                    loop {
                        match pipe.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let room = 8192usize.saturating_sub(buf.len());
                                buf.extend_from_slice(&chunk[..n.min(room)]);
                            }
                        }
                    }
                    buf
                })
            })
        } else {
            let _stderr = child.stderr.take();
            None
        };

        // ── Phase 1: drain initial noise (banner, MOTD, prompt) ──
        // In batch mode this is also where a bastion login prompt would appear.
        let mut head: Vec<u8> = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            match tokio::time::timeout(Duration::from_secs(1), stdout.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => {
                    if extras.non_interactive && head.len() < 16 * 1024 {
                        let room = 16 * 1024 - head.len();
                        head.extend_from_slice(&chunk[..n.min(room)]);
                        if let Some(prompt) = auth_prompt_kind(&String::from_utf8_lossy(&head)) {
                            let _ = child.kill().await;
                            return Err(prompt_error(conn, prompt));
                        }
                    }
                }
                _ => break,
            }
        }

        // ── Phase 2: send markers + command ──
        // One compound command: the shell reads (and its line editor echoes and
        // redraws) the whole thing before running any of it, so only the
        // command's own output can land between the markers, whatever the prompt
        // looks like. Each marker is split in the source (`'X''_START'`) so the
        // echoed command line never contains it. The PTY is widened so
        // column-aware tools don't wrap at 80.
        let payload = interactive_payload(&marker, command);
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
        let ssh_stderr = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => Vec::new(),
        };

        let head_txt = String::from_utf8_lossy(&head);
        let raw = String::from_utf8_lossy(&raw_bytes);
        let (clean_lines, saw_start, saw_end) =
            extract_between_markers(&raw, &marker_start, &marker_end);

        if extras.non_interactive && !saw_start {
            let combined = format!(
                "{head_txt}\n{raw}\n{}",
                String::from_utf8_lossy(&ssh_stderr)
            );
            if needs_tty(combined.as_bytes(), b"")
                || combined.contains("Veuillez ajouter l'option -t")
            {
                return Err(TransportError::permanent(anyhow::anyhow!(
                    "the bastion refused the session (asked for -t even though a PTY was \
                     requested). If this agent runs with TERM=dumb, that is usually why; \
                     bssh overrides it for ssh, so retry — or run `bssh connect {}` once \
                     to confirm the bastion still accepts you",
                    conn.name
                )));
            }
            if combined.contains("Permission denied") {
                let fake = LimitedOutput {
                    stdout: Vec::new(),
                    stderr: combined.into_bytes(),
                    exit_code: Some(255),
                    truncated: false,
                    timed_out: false,
                };
                if let Some(err) = classify_ssh_failure(&fake, Some(conn)) {
                    return Err(err);
                }
            }
            return Err(TransportError::permanent(anyhow::anyhow!(
                "no shell on {} through the bastion (command markers never appeared). \
                 Check the Kerberos ticket (`klist` / `kinit`) and that agent forwarding \
                 is enabled on the connection when the target needs your key",
                conn.name
            )));
        }

        let stdout_bytes = if saw_start {
            let mut joined = clean_lines.join("\n");
            if saw_end || !clean_lines.is_empty() {
                joined.push('\n');
            }
            joined.into_bytes()
        } else if clean_lines.is_empty() {
            // Human / plain path: keep prior behaviour when markers are missing.
            raw_bytes
        } else {
            let mut joined = clean_lines.join("\n");
            joined.push('\n');
            joined.into_bytes()
        };

        Ok(ExecOutput {
            stdout: stdout_bytes,
            // The remote command's stderr is in stdout (PTY). What remains here is
            // ssh's own diagnostics, kept only to classify a failure: once the markers
            // came back the session worked, and it is just the bastion's login banner.
            stderr: if saw_start { Vec::new() } else { ssh_stderr },
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

/// The text sent to an interactive bastion's shell: one compound command whose
/// markers are split in the source, so the echoed line can never contain them.
/// `{ … \n}` lets `command` span lines (heredocs) and end in a comment.
fn interactive_payload(marker: &str, command: &str) -> String {
    format!(
        "stty cols 200 rows 50 2>/dev/null; stty -echo 2>/dev/null; \
         echo '{marker}''_START'; {{ {command}\n}}; _bssh_rc=$?; \
         echo '{marker}''_END'; exit $_bssh_rc\n"
    )
}

/// The command's output lines between the PTY markers, and whether each marker showed up.
///
/// Everything before START (banner, MOTD, the echoed command) and after END is dropped.
/// Output that does not end in a newline puts the END marker on its last line
/// (`dataBSSH_…_END`): the text before the marker is output and is kept.
fn extract_between_markers<'a>(
    raw: &'a str,
    marker_start: &str,
    marker_end: &str,
) -> (Vec<&'a str>, bool, bool) {
    let mut capture = false;
    let mut saw_start = false;
    let mut saw_end = false;
    let mut clean_lines: Vec<&'a str> = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if !saw_start {
            if line.trim_end().ends_with(marker_start) {
                capture = true;
                saw_start = true;
            }
            continue;
        }
        if !capture {
            break;
        }
        if let Some(before) = line.trim_end().strip_suffix(marker_end) {
            if !before.is_empty() {
                clean_lines.push(before);
            }
            capture = false;
            saw_end = true;
            continue;
        }
        clean_lines.push(line);
    }
    (clean_lines, saw_start, saw_end)
}

/// Did `ssh` report that the server needs a pseudo-terminal? (Same check as plain `exec`.)
fn needs_tty(stdout: &[u8], stderr: &[u8]) -> bool {
    [stdout, stderr].iter().any(|bytes| {
        let s = String::from_utf8_lossy(bytes);
        s.contains("option -t") || s.contains("l'option -t") || s.contains("pseudo-terminal")
    })
}

/// What the bastion is waiting for, when an authentication prompt shows up on the PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthPrompt {
    Password,
    OneTimeCode,
}

impl AuthPrompt {
    /// The words used in the agent-facing error.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            AuthPrompt::Password => "a password",
            AuthPrompt::OneTimeCode => "a one-time code",
        }
    }
}

/// Case-insensitive prompt detection for text that arrived over the bastion's PTY.
///
/// An agent cannot answer any of these, and waiting would only burn the tool timeout,
/// so the caller kills `ssh` as soon as this matches.
pub(crate) fn auth_prompt_kind(text: &str) -> Option<AuthPrompt> {
    let lower = text.to_ascii_lowercase();
    // Check the one-time variants first: "Password or passcode:" is a code prompt.
    const ONE_TIME: &[&str] = &[
        "passcode",
        "verification code",
        "otp",
        "one-time",
        "mfa",
        "2fa",
        "enter pin",
    ];
    if ONE_TIME.iter().any(|needle| lower.contains(needle)) {
        return Some(AuthPrompt::OneTimeCode);
    }
    if lower.contains("password:") {
        return Some(AuthPrompt::Password);
    }
    None
}

/// A certificate makes the *target* login bssh-agent; the bastion still authenticates as
/// the user, and its command syntax cannot start an agent session.
fn cert_through_kerberos_bastion(conn: &Connection) -> TransportError {
    TransportError::permanent(anyhow::anyhow!(
        "{} is behind a Kerberos bastion, which logs in as you; \
         certificate (bssh-agent) logins cannot pass through it. \
         Commands on this host run as your user",
        conn.name
    ))
}

/// The error for a bastion that answered with a prompt instead of a shell.
fn prompt_error(conn: &Connection, prompt: AuthPrompt) -> TransportError {
    let bastion = conn.bastion.as_deref().unwrap_or(&conn.host);
    TransportError::permanent(anyhow::anyhow!(
        "the bastion {bastion} asks for {}; agents cannot answer prompts. \
         Use a key, ssh-agent or a Kerberos ticket for the bastion",
        prompt.describe()
    ))
}

/// Fail before `ssh` starts when this host authenticates with Kerberos and has no ticket:
/// `ssh` would otherwise report a key/agent error that hides the real cause.
async fn require_kerberos_ticket(conn: &Connection) -> Result<(), TransportError> {
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TokioCommand::new("klist")
            .arg("-s")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    match status {
        Ok(Ok(s)) if s.success() => Ok(()),
        // No ticket, or `klist` is unusable: either way ssh cannot get one either.
        Ok(Ok(_)) | Err(_) => {
            let where_ = if conn.bastion.is_some() {
                "for the bastion".to_string()
            } else {
                format!("for {}", conn.host)
            };
            Err(TransportError::permanent(anyhow::anyhow!(
                "no valid Kerberos ticket {where_}; run kinit (or bssh connect {} once) and retry",
                conn.name
            )))
        }
        // `klist` is not installed: let ssh decide, it reports its own gssapi failure.
        Ok(Err(_)) => Ok(()),
    }
}

/// Create the run's `ControlPath` directory (0700) before the first `ssh` uses it.
fn prepare_control_dir(dir: &std::path::Path) -> Result<(), TransportError> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| TransportError::permanent(anyhow::anyhow!("{}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| TransportError::permanent(anyhow::anyhow!("{}: {e}", dir.display())))?;
    }
    Ok(())
}

impl SubprocessTransport {
    /// PTY-scraping exec under limits: the timeout wraps the whole run, the cap applies
    /// afterwards (the output is collected line-wise, it cannot stream).
    async fn limited_interactive(
        &self,
        conn: &Connection,
        command: &str,
        opts: &ExecOptions,
    ) -> Result<LimitedOutput, TransportError> {
        let extras = ShellExtras {
            non_interactive: opts.non_interactive,
            connect_timeout_secs: opts.connect_timeout.map(|t| t.as_secs().max(1)),
        };
        let run = self.run_interactive_exec_ext(conn, command, &extras);
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
        let limited = LimitedOutput::from_output(out, opts.max_output_bytes);
        if opts.non_interactive {
            if let Some(err) = classify_ssh_failure(&limited, Some(conn)) {
                return Err(err);
            }
        }
        Ok(limited)
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

        let output = prepare_ssh_command(cmd_name)
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
        let out = run_argv_limited(cmd_name, args, opts).await?;
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

        let mut child = prepare_ssh_command(cmd_name)
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

        let mut child = prepare_ssh_command(cmd_name)
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
            let mut child = prepare_ssh_command(cmd_name)
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
        Connection {
            bastion: bastion.map(String::from),
            use_kerberos: kerb,
            key_path: key.map(String::from),
            ..Connection::new("x".into(), "target.example".into(), "alice".into(), 2222)
        }
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
    fn exec_argv_never_forces_tty() {
        for bastion in [None, Some("b.example")] {
            let argv = SubprocessTransport::build_exec_argv(
                &c(false, bastion, None),
                "uptime",
                "accept-new",
            );
            assert!(argv.contains(&"-T".to_string()));
            assert!(!argv.contains(&"-t".to_string()));
            assert!(!argv.contains(&"-tt".to_string()));
            assert!(!argv.iter().any(|a| a.contains("RequestTTY=force")));
            assert!(!argv.iter().any(|a| a.contains(" -tt")));
        }
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
    fn ssh_shkc_value_accepts_ssh_aliases_and_fails_closed() {
        assert_eq!(ssh_strict_host_key_value("strict"), "yes");
        assert_eq!(ssh_strict_host_key_value("YES"), "yes");
        assert_eq!(ssh_strict_host_key_value("accept-new"), "accept-new");
        assert_eq!(ssh_strict_host_key_value("off"), "no");
        assert_eq!(ssh_strict_host_key_value("no"), "no");
        // A typo must not degrade to accept-new (which trusts an unknown host).
        assert_eq!(ssh_strict_host_key_value("accept_new"), "yes");
    }

    #[test]
    fn shell_argv_simple() {
        let argv =
            SubprocessTransport::build_shell_argv(&c(false, None, None), HostKeyPolicy::AcceptNew);
        assert_eq!(argv[0], "ssh");
        assert!(argv.contains(&"-p".to_string()));
        assert!(argv.contains(&"2222".to_string()));
        assert!(argv.last().unwrap().contains("alice@target.example"));
    }

    #[test]
    fn shell_argv_kerberos_adds_flags() {
        let argv =
            SubprocessTransport::build_shell_argv(&c(true, None, None), HostKeyPolicy::AcceptNew);
        assert!(argv.contains(&"-tt".to_string()));
        assert!(argv.contains(&"-K".to_string()));
        // Agent forwarding is a per-connection opt-in, not a Kerberos side effect.
        assert!(!argv.contains(&"-A".to_string()));
    }

    #[test]
    fn shell_argv_forwards_the_agent_only_when_opted_in() {
        let mut conn = c(true, None, None);
        conn.forward_agent = true;
        let argv = SubprocessTransport::build_shell_argv(&conn, HostKeyPolicy::AcceptNew);
        let a = argv.iter().position(|x| x == "-A");
        let k = argv.iter().position(|x| x == "-K");
        assert!(a.is_some() && k.is_some() && a < Some(k.unwrap()));

        // Without Kerberos, -A alone still needs the opt-in.
        let plain =
            SubprocessTransport::build_shell_argv(&c(false, None, None), HostKeyPolicy::AcceptNew);
        assert!(!plain.contains(&"-A".to_string()));

        let mut keyed = c(false, None, None);
        keyed.forward_agent = true;
        let argv = SubprocessTransport::build_shell_argv(&keyed, HostKeyPolicy::AcceptNew);
        assert!(argv.contains(&"-A".to_string()));
        assert!(!argv.contains(&"-K".to_string()));
    }

    #[test]
    fn exec_argv_never_forwards_the_agent() {
        for forward_agent in [false, true] {
            let mut conn = c(true, None, None);
            conn.forward_agent = forward_agent;
            let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
            assert!(!argv.contains(&"-A".to_string()), "{argv:?}");
            assert!(argv.contains(&"-K".to_string()));
        }
    }

    #[test]
    fn interactive_bastion_uses_the_stored_port() {
        let plain = SubprocessTransport::build_shell_argv(
            &c(true, Some("b.example"), None),
            HostKeyPolicy::AcceptNew,
        );
        assert!(!plain.contains(&"-p".to_string()), "{plain:?}");
        assert!(plain.contains(&"alice@b.example".to_string()));

        let ported = SubprocessTransport::build_shell_argv(
            &c(true, Some("b.example:2222"), None),
            HostKeyPolicy::AcceptNew,
        );
        let p = ported.iter().position(|a| a == "-p").expect("-p present");
        assert_eq!(ported[p + 1], "2222");
        assert!(
            ported.contains(&"alice@b.example".to_string()),
            "{ported:?}"
        );
    }

    /// The bastion hop is the one bssh itself verifies, so it must follow the
    /// configured policy instead of a hard-coded `accept-new`.
    #[test]
    fn shell_argv_bastion_hop_honours_the_configured_policy() {
        let conn = c(false, Some("b.example"), None);
        let expected = [
            (HostKeyPolicy::Strict, "StrictHostKeyChecking=yes"),
            (HostKeyPolicy::AcceptNew, "StrictHostKeyChecking=accept-new"),
            (HostKeyPolicy::Off, "StrictHostKeyChecking=no"),
        ];
        for (policy, option) in expected {
            let argv = SubprocessTransport::build_shell_argv(&conn, policy);
            let proxy = argv
                .iter()
                .find(|a| a.starts_with("ProxyCommand="))
                .expect("ProxyCommand present");
            assert!(proxy.contains(option), "{policy:?}: {proxy}");
            // The target prompt is still the user's ssh's (documented behaviour).
            assert!(
                !argv.iter().any(|a| a == option),
                "{policy:?} leaks to target"
            );
        }
    }

    #[test]
    fn shell_argv_direct_connection_leaves_target_prompt_to_ssh() {
        // No bastion: nothing is enforced, OpenSSH's interactive `ask` applies.
        for policy in [
            HostKeyPolicy::Strict,
            HostKeyPolicy::AcceptNew,
            HostKeyPolicy::Off,
        ] {
            let argv = SubprocessTransport::build_shell_argv(&c(false, None, None), policy);
            assert!(
                !argv.iter().any(|a| a.starts_with("StrictHostKeyChecking=")),
                "{policy:?}: {argv:?}"
            );
        }
    }

    #[test]
    fn destination_follows_double_dash() {
        let mut conn = c(false, None, None);
        conn.user = "-oProxyCommand=touch /tmp/pwned".into();
        let argv = SubprocessTransport::build_exec_argv(&conn, "uptime", "accept-new");
        let dd = argv.iter().position(|a| a == "--").expect("-- present");
        assert!(argv[dd + 1].starts_with("-oProxyCommand"));
        let shell = SubprocessTransport::build_shell_argv(&conn, HostKeyPolicy::AcceptNew);
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
        let proxy = jump_proxy_command(&conn, "", "accept-new", &HopOptions::default());
        assert!(proxy.contains("-i /keys/100%%key"));
    }

    #[test]
    fn hop_argv_is_batch_and_bounded_when_non_interactive() {
        let conn = c(false, Some("b.example"), Some("/keys/id_ed25519"));
        let hop = HopOptions {
            non_interactive: true,
            connect_timeout_secs: Some(9),
        };
        let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
        assert!(proxy.contains("-o BatchMode=yes"), "{proxy}");
        assert!(proxy.contains("-o ConnectTimeout=9"), "{proxy}");
        // The hop logs in as the connection's own user with its own key.
        assert!(proxy.contains("-i /keys/id_ed25519"), "{proxy}");
        assert!(proxy.ends_with("-- alice@b.example"), "{proxy}");

        // Interactive CLI hops keep prompting: no batch switches.
        let cli = jump_proxy_command(&conn, "", "accept-new", &HopOptions::default());
        assert!(!cli.contains("BatchMode"), "{cli}");
        assert!(!cli.contains("ConnectTimeout"), "{cli}");
    }

    #[test]
    fn cert_hops_keep_the_original_bastion_identity() {
        // `exec.rs::agent_connection` swaps the target login to bssh-agent and keeps the
        // original bastion user (and the original key for the hop only).
        let mut conn = c(
            false,
            Some("b.example"),
            Some("/home/alice/.ssh/id_ed25519"),
        );
        conn.bastion_user = Some("ops".into());
        conn.user = "bssh-agent".into();
        let hop = HopOptions {
            non_interactive: true,
            connect_timeout_secs: Some(5),
        };
        let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
        assert!(proxy.ends_with("-- ops@b.example"), "{proxy}");

        // Without an explicit bastion user the raw helper falls back to `conn.user`;
        // exec.rs sets `bastion_user` before it calls this, which the exec test pins.
        conn.bastion_user = None;
        conn.user = "bssh-agent".into();
        let proxy = jump_proxy_command(&conn, " -T", "accept-new", &hop);
        assert!(proxy.ends_with("-- bssh-agent@b.example"), "{proxy}");
    }

    #[test]
    fn cert_exec_argv_keeps_the_hop_key_and_offers_only_the_certificate() {
        let mut conn = c(
            false,
            Some("b.example"),
            Some("/home/alice/.ssh/id_ed25519"),
        );
        conn.bastion_user = Some("alice".into());
        let extras = ArgvExtras {
            cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
            ..batch_extras()
        };
        let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &extras);
        let proxy = argv
            .iter()
            .find(|a| a.starts_with("ProxyCommand="))
            .expect("ProxyCommand");
        assert!(proxy.contains("-i /home/alice/.ssh/id_ed25519"), "{proxy}");
        assert!(proxy.contains("BatchMode=yes"), "{proxy}");
        assert!(proxy.ends_with("-- alice@b.example"), "{proxy}");
        // The target still authenticates with the certificate only.
        let target_keys: Vec<&String> = argv
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-i")
            .map(|(i, _)| &argv[i + 1])
            .collect();
        assert_eq!(target_keys, vec!["/t/id"]);
    }

    fn batch_extras() -> ArgvExtras {
        ArgvExtras {
            non_interactive: true,
            connect_timeout_secs: Some(7),
            cert: None,
            control_dir: None,
        }
    }

    #[test]
    fn default_exec_argv_has_no_batch_options() {
        let argv =
            SubprocessTransport::build_exec_argv(&c(false, None, None), "uptime", "accept-new");
        assert!(!argv.iter().any(|a| a.starts_with("BatchMode")));
        assert!(!argv.iter().any(|a| a.starts_with("ConnectTimeout")));
        assert!(!argv.contains(&"-a".to_string()));
    }

    #[test]
    fn batch_argv_never_prompts_and_never_forwards() {
        let argv = SubprocessTransport::build_exec_argv_with(
            &c(false, None, None),
            "uptime",
            "accept-new",
            &batch_extras(),
        );
        let has = |s: &str| argv.iter().any(|a| a == s);
        assert!(has("BatchMode=yes"));
        assert!(has("ConnectTimeout=7"));
        assert!(has("ClearAllForwardings=yes"));
        assert!(has("-a") && has("-x") && has("-T"));
        // The command is still the last word, after `--` and the destination.
        let n = argv.len();
        assert_eq!(argv[n - 1], "uptime");
        assert_eq!(argv[n - 2], "alice@target.example");
        assert_eq!(argv[n - 3], "--");
    }

    #[test]
    fn cert_argv_offers_only_the_certificate() {
        let extras = ArgvExtras {
            cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
            ..batch_extras()
        };
        // A per-connection key must not be offered next to the certificate.
        let argv = SubprocessTransport::build_exec_argv_with(
            &c(false, None, Some("/home/alice/.ssh/id_rsa")),
            "id",
            "accept-new",
            &extras,
        );
        assert!(argv.contains(&"IdentitiesOnly=yes".to_string()));
        assert!(argv.contains(&"CertificateFile=/t/id-cert.pub".to_string()));
        let keys: Vec<&String> = argv
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-i")
            .map(|(i, _)| &argv[i + 1])
            .collect();
        assert_eq!(keys, vec!["/t/id"]);
    }

    fn out(exit: Option<i32>, stderr: &str) -> LimitedOutput {
        LimitedOutput {
            stderr: stderr.as_bytes().to_vec(),
            exit_code: exit,
            ..LimitedOutput::default()
        }
    }

    #[test]
    fn permission_denied_becomes_an_actionable_error() {
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "alice@host: Permission denied (publickey,password).\n",
            ),
            None,
        )
        .expect("ssh failure")
        .to_string();
        assert!(
            err.contains("needs interactive auth; use a key or ssh-agent"),
            "{err}"
        );
        assert!(err.contains("Permission denied"), "{err}");
    }

    #[test]
    fn a_refused_bastion_login_names_the_bastion() {
        let mut conn = c(false, Some("bastion.corp"), None);
        conn.bastion_user = Some("alice".into());
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "alice@bastion.corp: Permission denied (publickey).\n",
            ),
            Some(&conn),
        )
        .expect("ssh failure")
        .to_string();
        assert!(
            err.contains("the bastion bastion.corp refused the login"),
            "{err}"
        );
        assert!(err.contains("alice@bastion.corp"), "{err}");
        assert!(!err.contains("needs interactive auth"), "{err}");

        // The target itself refusing a key keeps the key/agent advice.
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "deploy@target.example: Permission denied (publickey).\n",
            ),
            Some(&conn),
        )
        .expect("ssh failure")
        .to_string();
        assert!(err.contains("needs interactive auth"), "{err}");
    }

    #[test]
    fn a_bastion_that_forbids_forwarding_suggests_an_interactive_bastion() {
        let mut conn = c(false, Some("bastion.corp"), None);
        conn.name = "web-1".into();
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "channel 0: open failed: administratively prohibited: open failed\n\
                 Connection closed by UNKNOWN port 65535\n",
            ),
            Some(&conn),
        )
        .expect("ssh failure")
        .to_string();
        assert!(err.contains("forbids TCP forwarding"), "{err}");
        assert!(err.contains("bssh edit web-1 --kerberos true"), "{err}");
    }

    #[test]
    fn a_gssapi_rejection_points_at_the_ticket() {
        let mut conn = c(true, Some("bastion.corp"), None);
        conn.bastion_user = Some("alice".into());
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "alice@bastion.corp: Permission denied (gssapi-with-mic,publickey).\n",
            ),
            Some(&conn),
        )
        .expect("ssh failure")
        .to_string();
        assert!(err.contains("Kerberos login failed"), "{err}");
        assert!(err.contains("klist") && err.contains("kinit"), "{err}");
    }

    #[test]
    fn other_ssh_failures_keep_their_message() {
        let err = classify_ssh_failure(
            &out(
                Some(255),
                "ssh: connect to host h port 22: Connection refused\n",
            ),
            None,
        )
        .expect("ssh failure")
        .to_string();
        assert!(err.contains("Connection refused"), "{err}");
    }

    #[test]
    fn remote_failures_are_results_not_errors() {
        // The remote command's own exit codes are data, even 255 without an ssh message.
        assert!(classify_ssh_failure(&out(Some(1), "Permission denied\n"), None).is_none());
        assert!(classify_ssh_failure(&out(Some(255), "boom\n"), None).is_none());
        assert!(classify_ssh_failure(&out(Some(0), ""), None).is_none());
    }

    #[test]
    fn auth_prompts_are_detected_case_insensitively() {
        assert_eq!(
            auth_prompt_kind("alice@bastion's Password: "),
            Some(AuthPrompt::Password)
        );
        assert_eq!(
            auth_prompt_kind("PASSCODE (or press Ctrl-C):"),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("Enter verification code: "),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("YubiKey OTP: "),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("One-time password: "),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("MFA required"),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("2FA token:"),
            Some(AuthPrompt::OneTimeCode)
        );
        assert_eq!(
            auth_prompt_kind("Enter PIN for Yubikey:"),
            Some(AuthPrompt::OneTimeCode)
        );
        // A banner or MOTD is not a prompt.
        assert_eq!(
            auth_prompt_kind("Welcome to bastion corp. Use of this system is logged.\n$ "),
            None
        );
    }

    #[test]
    fn the_prompt_error_names_the_bastion_and_the_way_out() {
        let err = prompt_error(
            &c(true, Some("bastion.corp"), None),
            AuthPrompt::OneTimeCode,
        )
        .to_string();
        assert!(
            err.contains("the bastion bastion.corp asks for a one-time code"),
            "{err}"
        );
        assert!(err.contains("agents cannot answer prompts"), "{err}");
        assert!(err.contains("key, ssh-agent or a Kerberos ticket"), "{err}");
    }

    #[test]
    fn a_certificate_through_a_kerberos_bastion_explains_why_it_cannot_pass() {
        let mut conn = c(true, Some("bastion.corp"), None);
        conn.name = "web-1".into();
        let err = cert_through_kerberos_bastion(&conn).to_string();
        assert!(err.contains("web-1 is behind a Kerberos bastion"), "{err}");
        assert!(
            err.contains("certificate (bssh-agent) logins cannot pass"),
            "{err}"
        );
        assert!(err.contains("run as your user"), "{err}");
    }

    #[test]
    fn shell_argv_for_an_agent_on_an_interactive_bastion_carries_target_credentials() {
        // Without forward_agent: BatchMode, no -A, but TGT delegation so the
        // bastion can GSSAPI-auth to the target.
        let conn = c(true, Some("bastion.corp"), None);
        let extras = ShellExtras {
            non_interactive: true,
            connect_timeout_secs: Some(12),
        };
        let argv = SubprocessTransport::build_shell_argv_ext(
            &conn,
            HostKeyPolicy::AcceptNew,
            true,
            &extras,
        );
        let has = |s: &str| argv.iter().any(|a| a == s);
        assert!(has("BatchMode=yes"), "{argv:?}");
        assert!(has("ConnectTimeout=12"), "{argv:?}");
        assert!(has("GSSAPIAuthentication=yes"), "{argv:?}");
        assert!(has("GSSAPIDelegateCredentials=yes"), "{argv:?}");
        assert!(!has("-K"), "flag form: {argv:?}");
        assert!(!has("-A"), "forward_agent is off: {argv:?}");
        assert!(has("ClearAllForwardings=yes"), "{argv:?}");
        let dd = argv.iter().position(|a| a == "--").expect("--");
        assert_eq!(argv[dd + 1], "alice@bastion.corp");
        assert_eq!(argv[dd + 2], "alice@target.example");

        // With forward_agent: -A is required when the target wants the user's key.
        let mut fwd = conn;
        fwd.forward_agent = true;
        let argv = SubprocessTransport::build_shell_argv_ext(
            &fwd,
            HostKeyPolicy::AcceptNew,
            true,
            &extras,
        );
        assert!(argv.iter().any(|a| a == "-A"), "{argv:?}");
        assert!(
            !argv.iter().any(|a| a == "ClearAllForwardings=yes"),
            "{argv:?}"
        );
        assert!(argv.iter().any(|a| a == "GSSAPIDelegateCredentials=yes"));
    }

    #[test]
    fn direct_kerberos_exec_argv_does_not_delegate() {
        let conn = c(true, None, None);
        let argv =
            SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &batch_extras());
        assert!(!argv.contains(&"-K".to_string()), "{argv:?}");
        assert!(argv.contains(&"GSSAPIAuthentication=yes".to_string()));
        assert!(argv.contains(&"GSSAPIDelegateCredentials=no".to_string()));
        // Interactive CLI exec keeps `-K` (a human may be watching).
        let cli = SubprocessTransport::build_exec_argv(&conn, "id", "accept-new");
        assert!(cli.contains(&"-K".to_string()), "{cli:?}");
    }

    #[test]
    fn control_master_is_only_for_batch_calls_without_a_certificate() {
        let conn = c(false, Some("b.example"), None);
        let dir = PathBuf::from("/run/bssh/ssh");
        let extras = ArgvExtras {
            control_dir: Some(dir.clone()),
            ..batch_extras()
        };
        let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &extras);
        assert!(argv.contains(&"ControlMaster=auto".to_string()), "{argv:?}");
        assert!(argv.contains(&"ControlPath=/run/bssh/ssh/%C".to_string()));
        assert!(argv.contains(&"ControlPersist=60".to_string()));

        // Certificates are one identity per command: sharing would reuse the wrong one.
        let cert = ArgvExtras {
            cert: Some((PathBuf::from("/t/id"), PathBuf::from("/t/id-cert.pub"))),
            ..batch_extras()
        };
        let argv = SubprocessTransport::build_exec_argv_with(&conn, "id", "accept-new", &cert);
        assert!(
            !argv.iter().any(|a| a.starts_with("ControlMaster")),
            "{argv:?}"
        );

        // A plain CLI exec never shares connections behind the user's back.
        let cli = SubprocessTransport::build_exec_argv(&conn, "id", "accept-new");
        assert!(
            !cli.iter().any(|a| a.starts_with("ControlMaster")),
            "{cli:?}"
        );
    }

    #[test]
    fn the_echoed_payload_never_contains_a_marker() {
        let p = interactive_payload("BSSH_00ff", "uptime");
        assert!(!p.contains("BSSH_00ff_START"), "{p}");
        assert!(!p.contains("BSSH_00ff_END"), "{p}");
        // One logical command, terminated once: the shell reads it all before running it.
        assert_eq!(p.matches('\n').count(), 2, "{p}");
        assert!(p.contains("{ uptime\n}"), "{p}");
    }

    #[test]
    fn only_output_between_the_markers_is_kept() {
        // Shaped like qupload01v: zsh with a coloured prompt, PROMPT_SP, and the line
        // editor echoing the whole payload before it runs.
        let raw = "banner\r\n\x1b[1m%\x1b[0m   \r \r\r<16:29>[cs-x:~]\x1b[0m$ u\x08uptime; echo 'M''_END'\r\r\n\
                   M_START\r\n 16:29:20 up 26 days\r\nM_END\r\n\x1b[1m%\x1b[0m  <16:29>[cs-x:~]$ \r\n";
        let (lines, start, end) = extract_between_markers(raw, "M_START", "M_END");
        assert!(start && end);
        assert_eq!(lines, vec![" 16:29:20 up 26 days"]);

        // Output with no final newline shares its line with the END marker.
        let raw = "M_START\r\nline one\r\naGk=M_END\r\n";
        let (lines, _, _) = extract_between_markers(raw, "M_START", "M_END");
        assert_eq!(lines, vec!["line one", "aGk="]);

        // No START marker: nothing is output (the caller turns this into an error).
        let (lines, start, _) = extract_between_markers("banner\r\nbye\r\n", "M_START", "M_END");
        assert!(!start && lines.is_empty());
    }

    #[test]
    fn needs_tty_detects_the_ssh_messages() {
        assert!(needs_tty(b"", b"sudo: you must have a pseudo-terminal"));
        assert!(needs_tty(b"sudo: option -t required", b""));
        assert!(!needs_tty(b"hello", b"world"));
    }

    #[cfg(unix)]
    mod run {
        use super::*;
        use std::time::{Duration, Instant};

        fn sh(script: &str, opts: &ExecOptions) -> LimitedOutput {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            rt.block_on(run_argv_limited(
                "sh",
                &["-c".to_string(), script.to_string()],
                opts,
            ))
            .expect("run")
        }

        #[test]
        fn collects_output_and_exit_code() {
            let out = sh("echo out; echo err >&2; exit 3", &ExecOptions::default());
            assert_eq!(out.stdout, b"out\n");
            assert_eq!(out.stderr, b"err\n");
            assert_eq!(out.exit_code, Some(3));
            assert!(!out.truncated && !out.timed_out);
        }

        #[test]
        fn timeout_kills_the_child_and_keeps_partial_output() {
            let opts = ExecOptions {
                timeout: Some(Duration::from_millis(400)),
                ..ExecOptions::default()
            };
            let started = Instant::now();
            let out = sh("echo partial; exec sleep 30", &opts);
            assert!(started.elapsed() < Duration::from_secs(10), "did not wait");
            assert!(out.timed_out);
            assert_eq!(out.exit_code, None);
            assert_eq!(out.stdout, b"partial\n");
        }

        #[test]
        fn timeout_leaves_no_child_running() {
            let dir = tempfile::tempdir().expect("tempdir");
            let pidfile = dir.path().join("pid");
            let script = format!("echo $$ > {}; exec sleep 30", pidfile.display());
            let opts = ExecOptions {
                timeout: Some(Duration::from_millis(500)),
                ..ExecOptions::default()
            };
            let out = sh(&script, &opts);
            assert!(out.timed_out);
            let pid = std::fs::read_to_string(&pidfile)
                .expect("pid file")
                .trim()
                .to_string();
            // The process is gone (or a zombie already reaped): /proc has no live entry.
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            assert!(
                stat.is_empty() || stat.contains(") Z "),
                "still running: {stat}"
            );
        }

        #[test]
        fn caps_each_stream_and_marks_truncated() {
            let opts = ExecOptions {
                max_output_bytes: Some(100),
                ..ExecOptions::default()
            };
            // 200 KB on stdout, 50 bytes on stderr: only stdout exceeds the cap.
            let out = sh(
                "head -c 200000 /dev/zero | tr '\\0' x; echo 'short error' >&2; exit 0",
                &opts,
            );
            assert_eq!(out.stdout.len(), 100);
            assert!(out.stdout.iter().all(|b| *b == b'x'));
            assert_eq!(out.stderr, b"short error\n");
            assert!(out.truncated);
            assert_eq!(
                out.exit_code,
                Some(0),
                "the command was drained, not killed"
            );
        }

        #[test]
        fn output_under_the_cap_is_not_truncated() {
            let opts = ExecOptions {
                max_output_bytes: Some(100),
                ..ExecOptions::default()
            };
            let out = sh("printf 'x%.0s' $(seq 1 100)", &opts);
            assert_eq!(out.stdout.len(), 100);
            assert!(!out.truncated, "exactly the cap fits");
        }

        #[test]
        fn missing_program_is_an_error() {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let res = rt.block_on(run_argv_limited(
                "/nonexistent/ssh-binary",
                &[],
                &ExecOptions::default(),
            ));
            assert!(res.is_err());
        }

        #[test]
        fn cert_files_are_private_and_removed() {
            use std::os::unix::fs::PermissionsExt;
            let cert = CertAuth {
                private_key_openssh: zeroize::Zeroizing::new("KEY".to_string()),
                certificate_openssh: "CERT".to_string(),
            };
            let (dir, key, crt) = write_cert_files(&cert).expect("write");
            let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode();
            assert_eq!(mode(&key) & 0o077, 0, "key readable by others");
            assert_eq!(mode(dir.path()) & 0o077, 0, "dir readable by others");
            assert_eq!(std::fs::read_to_string(&crt).unwrap(), "CERT");
            let root = dir.path().to_path_buf();
            drop(dir);
            assert!(!root.exists());
        }
    }
}
