//! Running `ssh` children under output caps, classifying `ssh`'s own
//! failures, and writing short-lived certificate files.

//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::models::Connection;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::argv::prepare_ssh_command;
use crate::services::transport::types::{
    CappedBuf, CertAuth, ExecOptions, LimitedOutput, TransportError,
};

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
pub(super) fn classify_ssh_failure(
    out: &LimitedOutput,
    conn: Option<&Connection>,
) -> Option<TransportError> {
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
    agent: Option<&Path>,
) -> Result<LimitedOutput, TransportError> {
    let mut child = prepare_ssh_command(program, agent)
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
pub(super) fn write_cert_files(
    cert: &CertAuth,
) -> anyhow::Result<(tempfile::TempDir, PathBuf, PathBuf)> {
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
