//! Exec through interactive (Kerberos) bastions: drive the bastion's shell
//! over a PTY, send the command between markers and scrape the result.

//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::models::Connection;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

use super::argv::{prepare_ssh_command, ShellExtras};
use super::process::classify_ssh_failure;
use super::SubprocessTransport;
use crate::services::transport::types::{ExecOptions, ExecOutput, LimitedOutput, TransportError};

impl SubprocessTransport {
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

        let mut child = prepare_ssh_command(cmd_name, self.config.auth.agent_socket.as_deref())
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
pub(super) fn interactive_payload(marker: &str, command: &str) -> String {
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
pub(super) fn extract_between_markers<'a>(
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
pub(super) fn needs_tty(stdout: &[u8], stderr: &[u8]) -> bool {
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
pub(super) fn cert_through_kerberos_bastion(conn: &Connection) -> TransportError {
    TransportError::permanent(anyhow::anyhow!(
        "{} is behind a Kerberos bastion, which logs in as you; \
         certificate (bssh-agent) logins cannot pass through it. \
         Commands on this host run as your user",
        conn.name
    ))
}

/// The error for a bastion that answered with a prompt instead of a shell.
pub(super) fn prompt_error(conn: &Connection, prompt: AuthPrompt) -> TransportError {
    let bastion = conn.bastion.as_deref().unwrap_or(&conn.host);
    TransportError::permanent(anyhow::anyhow!(
        "the bastion {bastion} asks for {}; agents cannot answer prompts. \
         Use a key, ssh-agent or a Kerberos ticket for the bastion",
        prompt.describe()
    ))
}

/// Fail before `ssh` starts when this host authenticates with Kerberos and has no ticket:
/// `ssh` would otherwise report a key/agent error that hides the real cause.
pub(super) async fn require_kerberos_ticket(conn: &Connection) -> Result<(), TransportError> {
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
pub(super) fn prepare_control_dir(dir: &std::path::Path) -> Result<(), TransportError> {
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
    pub(super) async fn limited_interactive(
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
        if opts.non_interactive
            && let Some(err) = classify_ssh_failure(&limited, Some(conn))
        {
            return Err(err);
        }
        Ok(limited)
    }
}
