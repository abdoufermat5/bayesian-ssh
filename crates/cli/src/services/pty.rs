//! Shared PTY primitives built on top of `portable-pty`.
//!
//! Both the desktop GUI (`crates/gui`) and the TUI/CLI can spawn, read,
//! write and resize a local PTY from a single definition. GUI-specific
//! concerns (Tauri event emission, detached-session buffering) live in the
//! GUI crate and pass callbacks into [`read_loop`].

#![allow(dead_code)]

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

/// Default terminal rows used when spawning a new PTY.
pub const DEFAULT_ROWS: u16 = 24;
/// Default terminal columns used when spawning a new PTY.
pub const DEFAULT_COLS: u16 = 80;
/// `TERM` value forced for xterm.js compatibility (vim/nano/htop rendering).
pub const TERM_ENV: &str = "xterm-256color";
/// Size of the chunk read from the PTY master in the read loop.
pub const READ_CHUNK_SIZE: usize = 4096;

/// Result of [`spawn`]: the handles needed to drive a PTY session.
pub struct SpawnedPty {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    /// Kept alive for the session duration so the PTY master stays valid.
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
}

/// Arguments for [`spawn`].
pub struct PtySpawnOptions<'a> {
    pub cmd_name: &'a str,
    pub args: &'a [String],
    pub rows: u16,
    pub cols: u16,
}

/// Spawn a command inside a fresh PTY.
///
/// Inherits the parent environment (so Kerberos `KRB5CCNAME` and
/// `SSH_AUTH_SOCK` are passed down to the child) and forces
/// `TERM=xterm-256color` for xterm.js compatibility.
pub fn spawn(options: PtySpawnOptions<'_>) -> Result<SpawnedPty, String> {
    let mut cmd_builder = CommandBuilder::new(options.cmd_name);
    cmd_builder.args(options.args);

    // Inherit env vars so Kerberos tickets (KRB5CCNAME) and ssh-agent
    // (SSH_AUTH_SOCK) are passed down.
    for (key, val) in std::env::vars() {
        cmd_builder.env(key, val);
    }

    // Ensure TERM is set for xterm.js compatibility — without this,
    // vim/nano/htop won't render.
    cmd_builder.env("TERM", TERM_ENV);

    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize {
            rows: options.rows,
            cols: options.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pty_pair
        .slave
        .spawn_command(cmd_builder)
        .map_err(|e| e.to_string())?;

    let reader = pty_pair
        .master
        .try_clone_reader()
        .map_err(|e| e.to_string())?;
    let writer = pty_pair.master.take_writer().map_err(|e| e.to_string())?;

    Ok(SpawnedPty {
        reader,
        writer,
        master: pty_pair.master,
        child,
    })
}

/// Write data to the PTY master and flush it immediately.
pub fn write_all(writer: &mut (dyn Write + Send), data: &str) -> Result<(), String> {
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

/// Resize the PTY master to the given columns/rows.
pub fn resize(master: &(dyn MasterPty + Send), cols: u16, rows: u16) -> Result<(), String> {
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

/// Read from the PTY master until EOF, passing each lossy-UTF-8 chunk to
/// `on_data`.
///
/// Stops on EOF (0 bytes) or read error, matching the previous GUI read
/// loop. Chunk size is [`READ_CHUNK_SIZE`].
pub fn read_loop(reader: &mut (dyn Read + Send), mut on_data: impl FnMut(String)) {
    let mut buf = [0u8; READ_CHUNK_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => on_data(String::from_utf8_lossy(&buf[..n]).into_owned()),
            Err(_) => break,
        }
    }
}
