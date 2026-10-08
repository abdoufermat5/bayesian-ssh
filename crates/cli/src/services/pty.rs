//! Shared PTY primitives built on top of `portable-pty`.
//!
//! Both the desktop GUI (`crates/gui`) and the CLI can spawn, read,
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
pub const READ_CHUNK_SIZE: usize = 8192;
/// Coalesce PTY output into batches of at least this many bytes before
/// handing it to the caller. High-throughput programs (`yes`, `make -j`,
/// `cat bigfile`) would otherwise generate thousands of tiny IPC events per
/// second and freeze the GUI.
pub const FLUSH_THRESHOLD_BYTES: usize = 32 * 1024;

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
    writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
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

/// Read from the PTY master until EOF, passing each batch of output to
/// `on_data`.
///
/// Output is coalesced: bytes accumulate until [`FLUSH_THRESHOLD_BYTES`] is
/// reached, or a *partial* read (usually the end of an interactive burst)
/// flushes early. That keeps latency low for interactive prompts while
/// avoiding an IPC event flood during high-throughput output.
///
/// Stops on EOF (0 bytes) or read error, matching the previous GUI read
/// loop. Any pending buffered output is flushed before stopping.
///
/// A multi-byte UTF-8 character split across two reads is carried over to
/// the next read instead of being decoded (and corrupted) on its own.
pub fn read_loop(reader: &mut (dyn Read + Send), mut on_data: impl FnMut(String)) {
    let mut buf = [0u8; READ_CHUNK_SIZE];
    let mut pending = String::new();
    let mut carry = Utf8Carry::default();
    loop {
        match reader.read(&mut buf) {
            Ok(n) if n > 0 => {
                carry.decode_into(&buf[..n], &mut pending);
                let partial_read = n < buf.len();
                if pending.len() >= FLUSH_THRESHOLD_BYTES || (partial_read && !pending.is_empty()) {
                    on_data(std::mem::take(&mut pending));
                }
            }
            // EOF or read error: flush what is left (a dangling partial
            // character can no longer complete, so decode it lossily).
            _ => {
                carry.finish_into(&mut pending);
                if !pending.is_empty() {
                    on_data(std::mem::take(&mut pending));
                }
                break;
            }
        }
    }
}

/// Incremental UTF-8 decoder holding back the incomplete trailing bytes of
/// a character (at most 3) until the next chunk arrives.
#[derive(Default)]
struct Utf8Carry {
    tail: Vec<u8>,
}

impl Utf8Carry {
    fn decode_into(&mut self, chunk: &[u8], out: &mut String) {
        let joined;
        let mut bytes = if self.tail.is_empty() {
            chunk
        } else {
            self.tail.extend_from_slice(chunk);
            joined = std::mem::take(&mut self.tail);
            joined.as_slice()
        };

        loop {
            match std::str::from_utf8(bytes) {
                Ok(valid) => {
                    out.push_str(valid);
                    return;
                }
                Err(e) => {
                    let (valid, rest) = bytes.split_at(e.valid_up_to());
                    // `valid_up_to` marks a verified UTF-8 prefix.
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match e.error_len() {
                        // Truncated character at the end: keep it for later.
                        None => {
                            self.tail.extend_from_slice(rest);
                            return;
                        }
                        // Genuinely invalid bytes: replace and keep decoding.
                        Some(len) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            bytes = &rest[len..];
                        }
                    }
                }
            }
        }
    }

    fn finish_into(&mut self, out: &mut String) {
        if !self.tail.is_empty() {
            out.push_str(&String::from_utf8_lossy(&self.tail));
            self.tail.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_chunks(chunks: &[&[u8]]) -> String {
        let mut carry = Utf8Carry::default();
        let mut out = String::new();
        for chunk in chunks {
            carry.decode_into(chunk, &mut out);
        }
        carry.finish_into(&mut out);
        out
    }

    #[test]
    fn multibyte_char_split_across_reads_is_preserved() {
        let text = "héllo ✓ 🚀";
        let bytes = text.as_bytes();
        for split in 0..=bytes.len() {
            let (a, b) = bytes.split_at(split);
            assert_eq!(decode_chunks(&[a, b]), text, "split at {split}");
        }
    }

    #[test]
    fn emoji_split_into_single_bytes() {
        let bytes = "🚀".as_bytes();
        let chunks: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(decode_chunks(&chunks), "🚀");
    }

    #[test]
    fn invalid_bytes_are_replaced() {
        assert_eq!(decode_chunks(&[b"a\xffb"]), "a\u{FFFD}b");
    }

    #[test]
    fn dangling_partial_char_at_eof_is_lossy() {
        assert_eq!(decode_chunks(&[b"ok\xe2\x9c"]), "ok\u{FFFD}");
    }

    #[test]
    fn read_loop_reassembles_split_chars() {
        struct Chunked(Vec<Vec<u8>>);
        impl Read for Chunked {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let chunk = self.0.remove(0);
                buf[..chunk.len()].copy_from_slice(&chunk);
                Ok(chunk.len())
            }
        }
        let bytes = "✓✓".as_bytes();
        let mut reader = Chunked(vec![bytes[..2].to_vec(), bytes[2..].to_vec()]);
        let mut out = String::new();
        read_loop(&mut reader, |s| out.push_str(&s));
        assert_eq!(out, "✓✓");
    }
}
