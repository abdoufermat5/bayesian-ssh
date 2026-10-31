//! File access for `read_file` and `edit_file`, behind a trait so tests use a fake.
//!
//! The route follows the transport dispatcher: a host that needs the system `ssh` (a
//! bastion, Kerberos, an `ssh_config` ProxyJump) reads and writes with `dd`/`base64`
//! over `exec_limited`, so the same bastion rules apply to files as to commands. A host
//! the built-in client can reach directly uses SFTP, with non-interactive auth: an MCP
//! server has no terminal for a password prompt.
//!
//! Neither route is jailed. Policy and deny paths are enforced by the caller before it
//! gets here.

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transport::{
    pick_kind, ExecOptions, LimitedOutput, RusshTransport, SshTransport, SubprocessTransport,
    TransportKind,
};
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use base64::Engine;
use std::time::Duration;
use tokio::sync::mpsc;

/// Longest wait for the TCP connect and authentication of one file operation.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Writing through a PTY is slower than a plain channel; a full read is 256 KiB.
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
/// Above this, the base64 payload does not belong on a command line.
const EXEC_WRITE_MAX: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct FileRead {
    /// Bytes from `offset`, at most `max_bytes`.
    pub data: Vec<u8>,
    /// Size of the whole file on the host.
    pub size: u64,
    /// Unix permission bits when known.
    pub mode: Option<u32>,
}

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait FileAccess: Send + Sync {
    async fn read(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead>;

    /// Replace the file's content (create if missing).
    async fn write(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()>;
}

/// SFTP when the built-in transport can reach the host; `ssh` + `dd`/`base64` when it cannot.
pub struct SftpFiles {
    config: AppConfig,
    native: RusshTransport,
    subprocess: SubprocessTransport,
}

impl SftpFiles {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
            native: RusshTransport::new(config.clone()),
            subprocess: SubprocessTransport::new(config.clone()),
        }
    }

    /// Does this host need the system `ssh` (so SFTP, which the subprocess transport
    /// does not provide, is not an option)?
    fn via_subprocess(&self, conn: &Connection) -> bool {
        matches!(pick_kind(conn, &self.config), TransportKind::Subprocess)
    }

    fn exec_options(&self, timeout: Duration, max_output_bytes: usize) -> ExecOptions {
        ExecOptions {
            timeout: Some(timeout),
            max_output_bytes: Some(max_output_bytes),
            connect_timeout: Some(CONNECT_TIMEOUT),
            non_interactive: true,
            cert_auth: None,
            control_dir: None,
        }
    }

    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
        timeout: Duration,
        cap: usize,
    ) -> Result<LimitedOutput> {
        let opts = self.exec_options(timeout, cap);
        self.subprocess
            .exec_limited(conn, command, &opts)
            .await
            .map_err(|e| anyhow!("{e}"))
    }

    async fn read_sftp(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead> {
        let opts = self.exec_options(READ_TIMEOUT, 0);
        let sftp = self
            .native
            .open_sftp_limited(conn, &opts)
            .await
            .map_err(|e| anyhow!("sftp: {e}"))?;
        let st = sftp
            .stat(path)
            .await
            .map_err(|e| anyhow!("stat {path}: {e}"))?;
        if st.is_dir {
            return Err(anyhow!("{path} is a directory"));
        }
        let want = offset.saturating_add(max_bytes as u64);
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        let reader = sftp.read_all(path, tx);
        let collector = async move {
            let mut buf: Vec<u8> = Vec::new();
            while let Some(chunk) = rx.recv().await {
                buf.extend_from_slice(&chunk);
                if buf.len() as u64 >= want {
                    break;
                }
            }
            buf
        };
        let (res, buf) = tokio::join!(reader, collector);
        // The reader fails when we stop listening after the range is full: that is fine.
        if let Err(e) = res {
            if (buf.len() as u64) < want && (buf.len() as u64) < st.size {
                return Err(anyhow!("read {path}: {e}"));
            }
        }
        let start = (offset as usize).min(buf.len());
        let end = (start + max_bytes).min(buf.len());
        Ok(FileRead {
            data: buf[start..end].to_vec(),
            size: st.size,
            mode: Some(st.mode & 0o7777),
        })
    }

    /// `stat` first so the caller learns the whole file's size, then read exactly the
    /// requested window. Both runs are ordinary commands, so the bastion rules apply.
    async fn read_exec(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead> {
        let quoted = sh_quote(path);
        let cmd = format!("stat -c '%s %a %f' -- {quoted}");
        let out = self.exec(conn, &cmd, CONNECT_TIMEOUT, 4096).await?;
        if out.timed_out {
            bail!("stat {path}: timed out");
        }
        if out.exit_code != Some(0) {
            bail!("stat {path}: {}", failure_detail(&out));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut fields = text.split_whitespace();
        let (Some(size), Some(mode), Some(raw)) = (fields.next(), fields.next(), fields.next())
        else {
            bail!("stat {path}: unexpected output");
        };
        let size: u64 = size
            .parse()
            .map_err(|_| anyhow!("stat {path}: unreadable size"))?;
        let mode: u32 =
            u32::from_str_radix(mode, 8).map_err(|_| anyhow!("stat {path}: unreadable mode"))?;
        let raw = u32::from_str_radix(raw.trim_start_matches("0x"), 16)
            .map_err(|_| anyhow!("stat {path}: unreadable file type"))?;
        if raw & 0o170000 == 0o040000 {
            bail!("{path} is a directory");
        }

        // `dd` reads the window; base64 keeps binary data intact through a PTY, whose
        // line discipline would otherwise rewrite bytes.
        let expect = (size.saturating_sub(offset)).min(max_bytes as u64) as usize;
        let b64_cap = expect.div_ceil(3) * 4 + 4096;
        let cmd = format!(
            "dd if={quoted} bs=1 skip={offset} count={expect} 2>/dev/null | base64"
        );
        let out = self.exec(conn, &cmd, READ_TIMEOUT, b64_cap).await?;
        if out.timed_out {
            bail!("read {path}: timed out");
        }
        if out.exit_code != Some(0) {
            bail!("read {path}: {}", failure_detail(&out));
        }
        if out.truncated {
            bail!("read {path}: output was cut off before it ended");
        }
        let encoded: Vec<u8> = out
            .stdout
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let data = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .map_err(|_| {
                anyhow!("read {path}: the host did not return a plain file (unexpected output)")
            })?;
        if data.len() < expect {
            bail!(
                "read {path}: got {} of {expect} bytes (can {} read it?)",
                data.len(),
                conn.user
            );
        }
        Ok(FileRead {
            data: data[..expect].to_vec(),
            size,
            mode: Some(mode & 0o7777),
        })
    }

    async fn write_exec(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        if data.len() > EXEC_WRITE_MAX {
            bail!(
                "write {path}: {} bytes is too large to send through ssh (limit {EXEC_WRITE_MAX}); \
                 upload the file with bssh instead",
                data.len()
            );
        }
        let quoted = sh_quote(path);
        let mode = mode.unwrap_or(0o644);
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        // A PTY handles 4 KiB lines; keep the heredoc well inside that.
        let mut payload = String::with_capacity(encoded.len() + encoded.len() / 512 + 2);
        for line in encoded.as_bytes().chunks(1024) {
            payload.push_str(std::str::from_utf8(line).unwrap_or_default());
            payload.push('\n');
        }
        // Same result as an SFTP write: the file replaced atomically, `mode` applied, and
        // the previous owner kept when this user may set it.
        let cmd = format!(
            "t={quoted}.bssh-$$.tmp; umask 077; base64 -d > \"$t\" <<'BSSH_PAYLOAD'\n{payload}BSSH_PAYLOAD\n\
             chmod {mode:o} -- \"$t\"; \
             [ -e {quoted} ] && chown --reference={quoted} \"$t\" 2>/dev/null; \
             mv -f -- \"$t\" {quoted}; _bssh_w=$?; \
             [ $_bssh_w -eq 0 ] || rm -f -- \"$t\"; (exit $_bssh_w)"
        );
        let out = self.exec(conn, &cmd, WRITE_TIMEOUT, 8192).await?;
        if out.timed_out {
            bail!("write {path}: timed out");
        }
        if out.exit_code != Some(0) {
            bail!("write {path}: {}", failure_detail(&out));
        }
        Ok(())
    }
}

#[async_trait]
impl FileAccess for SftpFiles {
    async fn read(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead> {
        if self.via_subprocess(conn) {
            return self.read_exec(conn, path, offset, max_bytes).await;
        }
        self.read_sftp(conn, path, offset, max_bytes).await
    }

    async fn write(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        if self.via_subprocess(conn) {
            return self.write_exec(conn, path, data, mode).await;
        }
        let opts = self.exec_options(WRITE_TIMEOUT, 0);
        let sftp = self
            .native
            .open_sftp_limited(conn, &opts)
            .await
            .map_err(|e| anyhow!("sftp: {e}"))?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
        let owned = data.to_vec();
        let feeder = async move {
            for chunk in owned.chunks(32 * 1024) {
                if tx.send(chunk.to_vec()).await.is_err() {
                    break;
                }
            }
        };
        let writer = sftp.write_all(path, 0, rx, mode.unwrap_or(0o644), None);
        let (res, ()) = tokio::join!(writer, feeder);
        res.map_err(|e| anyhow!("write {path}: {e}"))?;
        Ok(())
    }
}

/// POSIX single-quote a path for a remote `sh`.
fn sh_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-./:@%+".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// One line for an error: ssh's (or the command's) own diagnostics, never the payload.
fn failure_detail(out: &LimitedOutput) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr.lines().map(str::trim).find(|l| !l.is_empty());
    match (out.exit_code, line) {
        (Some(code), Some(line)) => format!("exit {code}: {line}"),
        (Some(code), None) => format!("exit {code}"),
        (None, Some(line)) => line.to_string(),
        (None, None) => "no exit status".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_quoted_for_the_remote_shell() {
        assert_eq!(sh_quote("/var/log/app.log"), "/var/log/app.log");
        assert_eq!(
            sh_quote("/etc/my app/x;rm -rf ~"),
            "'/etc/my app/x;rm -rf ~'"
        );
        assert_eq!(sh_quote("/a'b"), "'/a'\"'\"'b'");
        assert_eq!(sh_quote(""), "''");
    }
}
