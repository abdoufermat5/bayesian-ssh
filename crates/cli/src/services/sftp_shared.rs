//! Shared subprocess-backed SFTP operations.
//!
//! The desktop GUI and CLI share a single implementation for remote
//! directory listing that shells out to the system `ssh` binary. The
//! pure-Rust `russh` SFTP subsystem lives separately in
//! [`crate::services::transport::sftp_impl`]; this module is the
//! subprocess-based path (works for direct connections, classic jump-host
//! bastions, and interactive Kerberos+bastion sessions).

#![allow(dead_code)]

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use crate::services::transport::subprocess_impl::shell_quote;
use serde::{Deserialize, Serialize};
use std::process::{Command, Output};

/// A remote directory entry surfaced to callers (serde round-trips to the
/// desktop frontend unchanged).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: String,
    pub modified: String,
}

/// Build the SSH argv for listing a remote directory.
/// Returns the argv and a bool indicating whether this is an interactive
/// bastion session (kerberos + bastion) that needs stdin/stdout piping.
fn build_ls_argv(conn: &Connection, path: &str, shkc: &str) -> (Vec<String>, bool) {
    let is_interactive_bastion = conn.use_kerberos && conn.bastion.is_some();

    let mut argv: Vec<String> = vec!["ssh".into()];

    if conn.use_kerberos {
        argv.push("-A".into());
        argv.push("-K".into());
    }
    if let Some(ref key_path) = conn.key_path {
        argv.push("-i".into());
        argv.push(key_path.clone());
    }
    argv.push("-o".into());
    argv.push(format!("StrictHostKeyChecking={shkc}"));
    argv.push("-o".into());
    argv.push("ConnectTimeout=15".into());

    if let Some(ref bastion) = conn.bastion {
        let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
        if conn.use_kerberos {
            // Interactive bastion: SSH into the bastion and pass target as argument.
            // No remote command in argv — we send it via stdin.
            argv.push("-tt".into());
            argv.push("-p".into());
            argv.push("22".into());
            argv.push(format!("{bu}@{bastion}"));
            argv.push(format!("{}@{}", conn.user, conn.host));
        } else {
            // Classic jump host via ProxyCommand (shell-quoted: the value is
            // executed through `sh -c`, so unquoted connection fields would
            // be a command-injection vector).
            let key_flag = if let Some(k) = &conn.key_path {
                format!(" -i {}", shell_quote(k))
            } else {
                String::new()
            };
            let proxy_cmd = format!(
                "ssh -o StrictHostKeyChecking={shkc}{key_flag} -W %h:%p {}@{}",
                shell_quote(bu),
                shell_quote(bastion)
            );
            argv.push("-o".into());
            argv.push(format!("ProxyCommand={proxy_cmd}"));
            argv.push("-p".into());
            argv.push(conn.port.to_string());
            argv.push(format!("{}@{}", conn.user, conn.host));
            // Remote command appended directly for non-interactive bastions and direct connections.
            let ls_cmd = build_ls_command(path);
            argv.push(ls_cmd);
        }
    } else {
        // Direct SSH connection.
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push(format!("{}@{}", conn.user, conn.host));
        let ls_cmd = build_ls_command(path);
        argv.push(ls_cmd);
    }

    (argv, is_interactive_bastion)
}

fn build_ls_command(path: &str) -> String {
    let escaped = path.replace('\'', "'\"'\"'");
    format!(
        "LC_ALL=C ls -la --time-style=long-iso -- '{escaped}' 2>/dev/null || LC_ALL=C ls -la -- '{escaped}'"
    )
}

/// Run ls on an interactive bastion (kerberos+bastion) using the marker technique.
///
/// Interactive bastions (Kerberos + bastion) only accept `target@host` as
/// an argument — they do NOT relay extra argv as a remote command.  We open
/// the SSH connection, wait for the initial banner/prompt to settle (drain
/// phase), then send our ls command wrapped between unique markers so we
/// can reliably extract the output.
fn run_interactive_ls(argv: &[String], path: &str) -> Result<Output, String> {
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    let (cmd_name, args) = argv
        .split_first()
        .ok_or("empty argv")?;

    let marker = format!("BSSH_SFTP_{:016x}", rand_marker());
    let marker_start = format!("{marker}_START");
    let marker_end = format!("{marker}_END");

    let ls_cmd = build_ls_command(path);

    let payload = format!(
        "stty cols 300 rows 50 2>/dev/null\necho '{marker_start}'\n{ls_cmd}\necho '{marker_end}'\nexit\n"
    );

    let mut child = std::process::Command::new(cmd_name)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn SSH: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin pipe")?;
    let mut stdout_pipe = child.stdout.take().ok_or("no stdout pipe")?;

    // ── Phase 1: drain initial noise (banner, MOTD, prompt) ──────────────────
    // Read in a background thread and relay chunks over a channel.
    // We stop draining after 1.5 s of silence (connection is ready).
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let drain_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match stdout_pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = tx.send(buf[..n].to_vec());
                }
                Err(_) => break,
            }
        }
        stdout_pipe
    });

    // Drain for up to 8 s total but stop after 1.5 s of silence.
    let drain_timeout = Duration::from_secs(8);
    let silence_timeout = Duration::from_millis(1500);
    let start = std::time::Instant::now();
    let mut drained: Vec<u8> = Vec::new();
    loop {
        match rx.recv_timeout(silence_timeout) {
            Ok(chunk) => {
                drained.extend_from_slice(&chunk);
                if start.elapsed() > drain_timeout {
                    break;
                }
            }
            Err(_) => break, // silence_timeout elapsed → connection is ready
        }
    }

    // ── Phase 2: send markers + command ───────────────────────────────────────
    stdin
        .write_all(payload.as_bytes())
        .map_err(|e| format!("Failed to write to SSH stdin: {e}"))?;
    drop(stdin);

    // Reclaim the stdout handle from the drain thread.
    let mut stdout_pipe = drain_handle.join().map_err(|_| "drain thread panicked")?;

    // ── Phase 3: read remaining output and extract between markers ─────────────
    // Start by prepending anything that was captured during the drain phase
    // (markers might have appeared there on very fast connections).
    let mut raw_stdout = drained;
    stdout_pipe
        .read_to_end(&mut raw_stdout)
        .map_err(|e| format!("Failed to read SSH stdout: {e}"))?;

    let mut raw_stderr = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_end(&mut raw_stderr);
    }

    let status = child.wait().map_err(|e| format!("Wait failed: {e}"))?;

    // Extract lines between markers
    let raw = String::from_utf8_lossy(&raw_stdout);
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
            // Skip PTY-echoed control commands (they have a shell prompt prefix)
            let has_prompt = trimmed.contains("% ") || trimmed.contains("$ ") || trimmed.contains("# ");
            let is_echo = has_prompt && (
                trimmed.contains("stty cols") ||
                trimmed.contains(&marker_start) ||
                trimmed.contains(&marker_end) ||
                trimmed.contains(&ls_cmd)
            );
            if !is_echo {
                clean_lines.push(line.trim_end_matches('\r'));
            }
        }
    }

    let stdout_bytes = if clean_lines.is_empty() {
        // If no markers found, fall back to raw output (useful for debugging)
        raw_stdout
    } else {
        let mut joined = clean_lines.join("\n");
        joined.push('\n');
        joined.into_bytes()
    };

    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: raw_stderr,
    })
}

/// Simple non-cryptographic random u64 from /dev/urandom for marker generation.
fn rand_marker() -> u64 {
    use std::io::Read;
    let mut buf = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    } else {
        // fallback: use time + pid
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
        let p = std::process::id() as u64;
        return t ^ (p << 32);
    }
    u64::from_ne_bytes(buf)
}

fn parse_sftp_output(output: &Output, path: &str) -> Result<Vec<RemoteFileEntry>, String> {
    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(if err_msg.trim().is_empty() {
            "Permission denied, SSH authentication failed, or directory does not exist.".to_string()
        } else {
            err_msg.to_string()
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("total ") || line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_dir = permissions.starts_with('d');
        let size = parts[4].parse::<u64>().unwrap_or(0);

        let (modified, name_parts_start) = if parts.len() >= 9 && parts[5].contains('-') {
            (format!("{} {}", parts[5], parts[6]), 7)
        } else if parts.len() >= 9 {
            (format!("{} {} {}", parts[5], parts[6], parts[7]), 8)
        } else {
            ("Unknown".to_string(), 7)
        };

        if name_parts_start >= parts.len() {
            continue;
        }

        let name = parts[name_parts_start..].join(" ");
        if name == "." || name == ".." {
            continue;
        }

        let full_path = if path == "." {
            name.clone()
        } else if path.ends_with('/') {
            format!("{path}{name}")
        } else {
            format!("{path}/{name}")
        };

        entries.push(RemoteFileEntry {
            name,
            path: full_path,
            is_dir,
            size,
            permissions,
            modified,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

/// List a remote directory for a stored connection.
///
/// Thin DB glue shared with the desktop GUI command; behavior (return shape,
/// entry fields, error strings) is identical to the previous GUI-local
/// implementation.
pub fn list_remote_directory(
    connection_name: &str,
    remote_path: &str,
) -> Result<Vec<RemoteFileEntry>, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;
    let conn = db
        .get_connection(connection_name)
        .map_err(|e| format!("Database error: {e}"))?
        .ok_or_else(|| format!("Connection '{connection_name}' not found."))?;

    let path = if remote_path.trim().is_empty() {
        ".".to_string()
    } else {
        remote_path.trim().to_string()
    };

    let shkc = "accept-new";
    let (argv, is_interactive) = build_ls_argv(&conn, &path, shkc);

    let output = if is_interactive {
        run_interactive_ls(&argv, &path)?
    } else {
        let (cmd_name, args) = argv
            .split_first()
            .ok_or("empty argv")?;
        Command::new(cmd_name)
            .args(args)
            .output()
            .map_err(|e| format!("SSH command failed: {e}"))?
    };

    parse_sftp_output(&output, &path)
}
