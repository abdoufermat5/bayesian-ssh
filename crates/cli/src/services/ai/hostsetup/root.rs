//! Running a script as root on a host, with or without a terminal.

use crate::config::HostKeyPolicy;
use crate::models::Connection;
use crate::services::ai::jail::shell_quote;
use crate::services::ai::types::{ExecLimits, ExecMode, ExecResult, Executor};
use crate::services::transport::SubprocessTransport;
use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::process::Stdio;
use std::time::Duration;

use super::scripts::EXIT_SUDO_PASSWORD;

/// What the root wrapper does when sudo wants a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SudoPassword {
    /// Exit with [`EXIT_SUDO_PASSWORD`] instead of waiting.
    Fail,
    /// Let sudo prompt on the attached terminal.
    Prompt,
}

/// One shell word that runs `script` as root: directly when we are root, else through
/// `sudo`. With [`SudoPassword::Fail`] it never waits for a password: it exits with
/// [`EXIT_SUDO_PASSWORD`] instead. The script is a positional argument, quoted once.
pub fn root_command(script: &str, sudo_password: SudoPassword) -> String {
    const WRAPPER: &str = r#"if [ "$(id -u)" -eq 0 ]; then exec sh -c "$1"; fi
command -v sudo >/dev/null 2>&1 || { echo 'bssh: not root and sudo is not installed' >&2; exit 78; }
if [ "$2" = n ]; then
  sudo -n true 2>/dev/null || { echo 'bssh: sudo needs a password' >&2; exit 77; }
  exec sudo -n sh -c "$1"
fi
exec sudo sh -c "$1"
"#;
    format!(
        "sh -c {} bssh-root {} {}",
        shell_quote(WRAPPER),
        shell_quote(script),
        match sudo_password {
            SudoPassword::Prompt => "i",
            SudoPassword::Fail => "n",
        }
    )
}

/// Result of [`run_root_script`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRun {
    pub exit_code: i32,
    /// Captured output; empty when the run was interactive (it went to the terminal).
    pub output: String,
    pub interactive: bool,
}

/// Run `script` as root on `conn`: first without a password prompt (root login or
/// passwordless sudo) through `exec`; if sudo asks for a password and `allow_interactive`
/// is set and a terminal is attached, once more over `ssh -tt` so the human types it.
///
/// `policy` is the configured host-key policy for the bastion hop of that
/// interactive fallback; the target's prompt stays the user's ssh's.
pub async fn run_root_script(
    conn: &Connection,
    exec: &dyn Executor,
    script: &str,
    allow_interactive: bool,
    policy: HostKeyPolicy,
) -> Result<RootRun> {
    let limits = ExecLimits {
        timeout: Duration::from_secs(180),
        max_output_bytes: 64 * 1024,
    };
    let res = exec
        .exec(
            conn,
            &root_command(script, SudoPassword::Fail),
            ExecMode::Plain,
            limits,
        )
        .await?;
    if res.exit_code != Some(EXIT_SUDO_PASSWORD) {
        return Ok(RootRun {
            exit_code: res.exit_code.unwrap_or(-1),
            output: combined_output(&res),
            interactive: false,
        });
    }
    if !allow_interactive || !std::io::stdin().is_terminal() {
        bail!(
            "sudo on {} needs a password and there is no terminal to ask on. \
             Run this command in a terminal, or allow passwordless sudo for this one run.",
            conn.name
        );
    }
    let code = run_root_script_interactive(conn, script, policy).await?;
    Ok(RootRun {
        exit_code: code,
        output: String::new(),
        interactive: true,
    })
}

async fn run_root_script_interactive(
    conn: &Connection,
    script: &str,
    policy: HostKeyPolicy,
) -> Result<i32> {
    if conn.use_kerberos && conn.bastion.is_some() {
        bail!("interactive sudo through a Kerberos bastion is not supported: log in and run the script by hand");
    }
    let mut argv = SubprocessTransport::build_shell_argv(conn, policy);
    argv.push(root_command(script, SudoPassword::Prompt));
    let (program, args) = argv.split_first().context("empty ssh command")?;
    let status = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("could not start {program}"))?;
    Ok(status.code().unwrap_or(-1))
}

fn combined_output(res: &ExecResult) -> String {
    let mut out = res.stdout.clone();
    if !res.stderr.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&res.stderr);
    }
    out
}
