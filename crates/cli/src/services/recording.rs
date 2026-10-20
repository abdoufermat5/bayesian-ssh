//! Session output recorder (asciicast v2) and stateful ANSI stripper for FTS5 indexing.

use crate::config::{AppConfig, RecordingConfig};
use crate::models::Connection;
use serde::Serialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

/// Maximum plain-text bytes buffered per session for the FTS5 index (4 MiB).
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// Final output event written once a recording reaches `max_bytes`.
const TRUNCATED_MARKER: &str = "\r\n[bssh: recording truncated]\r\n";

pub type SharedRecorder = Arc<Mutex<Recorder>>;

/// Decide whether a session to `conn` should be recorded.
pub fn should_record(conn: &Connection, cfg: &RecordingConfig, cli_override: Option<bool>) -> bool {
    if let Some(override_flag) = cli_override {
        return override_flag;
    }
    if !cfg.enabled {
        return false;
    }
    let filter_tags: Vec<String> = cfg
        .tags
        .iter()
        .filter_map(|t| Connection::normalize_tag(t))
        .collect();
    if filter_tags.is_empty() {
        return true;
    }
    conn.tags
        .iter()
        .filter_map(|t| Connection::normalize_tag(t))
        .any(|ct| filter_tags.iter().any(|ft| ft == &ct))
}

/// Directory holding `.cast` files for the active environment.
pub fn recordings_dir(config: &AppConfig) -> PathBuf {
    config
        .database_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("recordings")
}

/// Path of the `.cast` file for `session_id`.
pub fn recording_path(config: &AppConfig, session_id: Uuid) -> PathBuf {
    recordings_dir(config).join(format!("{session_id}.cast"))
}

#[derive(Serialize)]
struct CastHeader<'a> {
    version: u8,
    width: u16,
    height: u16,
    timestamp: i64,
    title: &'a str,
    env: CastEnv<'a>,
}

#[derive(Serialize)]
struct CastEnv<'a> {
    #[serde(rename = "TERM")]
    term: &'a str,
}

#[derive(Default)]
enum StripState {
    #[default]
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
    StrSeq,
    StrSeqEsc,
}

#[derive(Default)]
struct AnsiStripper {
    state: StripState,
    last_was_cr: bool,
}

impl AnsiStripper {
    fn feed(&mut self, input: &str, out: &mut String) {
        for ch in input.chars() {
            if out.len() >= MAX_TEXT_BYTES {
                break;
            }
            match self.state {
                StripState::Normal => match ch {
                    '\x1b' => {
                        self.last_was_cr = false;
                        self.state = StripState::Esc;
                    }
                    '\r' => {
                        self.last_was_cr = true;
                        Self::push_capped(out, '\n');
                    }
                    '\n' => {
                        if !self.last_was_cr {
                            Self::push_capped(out, '\n');
                        }
                        self.last_was_cr = false;
                    }
                    '\t' => {
                        self.last_was_cr = false;
                        Self::push_capped(out, '\t');
                    }
                    c if c < '\x20' || c == '\x7f' => {
                        self.last_was_cr = false;
                    }
                    c => {
                        self.last_was_cr = false;
                        Self::push_capped(out, c);
                    }
                },
                StripState::Esc => match ch {
                    '[' => self.state = StripState::Csi,
                    ']' => self.state = StripState::Osc,
                    'P' | '_' | '^' | 'X' => self.state = StripState::StrSeq,
                    _ => self.state = StripState::Normal,
                },
                StripState::Csi => match ch {
                    '\x1b' => self.state = StripState::Esc,
                    '\x40'..='\x7e' => self.state = StripState::Normal,
                    _ => {}
                },
                StripState::Osc => match ch {
                    '\x07' => self.state = StripState::Normal,
                    '\x1b' => self.state = StripState::OscEsc,
                    _ => {}
                },
                StripState::OscEsc => match ch {
                    '\\' | '\x07' => self.state = StripState::Normal,
                    '\x1b' => self.state = StripState::OscEsc,
                    '[' => self.state = StripState::Csi,
                    ']' => self.state = StripState::Osc,
                    _ => self.state = StripState::Normal,
                },
                StripState::StrSeq => {
                    if ch == '\x1b' {
                        self.state = StripState::StrSeqEsc;
                    }
                }
                StripState::StrSeqEsc => match ch {
                    '\\' => self.state = StripState::Normal,
                    '\x1b' => self.state = StripState::StrSeqEsc,
                    _ => self.state = StripState::StrSeq,
                },
            }
        }
    }

    fn push_capped(out: &mut String, ch: char) {
        if out.len() + ch.len_utf8() <= MAX_TEXT_BYTES {
            out.push(ch);
        }
    }
}

pub struct Recorder {
    writer: Option<BufWriter<File>>,
    start: Instant,
    bytes: u64,
    max_bytes: u64,
    truncated: bool,
    stripper: AnsiStripper,
    text: String,
}

impl Recorder {
    pub fn create(
        path: &Path,
        cols: u16,
        rows: u16,
        title: &str,
        max_bytes: u64,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
            crate::config::enforce_secure_dir(parent);
        }

        let file = File::create(path)?;
        crate::config::enforce_secure_file(path);
        let mut writer = BufWriter::new(file);

        let header = CastHeader {
            version: 2,
            width: cols,
            height: rows,
            timestamp: chrono::Utc::now().timestamp(),
            title,
            env: CastEnv {
                term: crate::services::pty::TERM_ENV,
            },
        };
        let mut line = serde_json::to_string(&header)?;
        line.push('\n');
        writer.write_all(line.as_bytes())?;
        writer.flush()?;

        Ok(Self {
            writer: Some(writer),
            start: Instant::now(),
            bytes: 0,
            max_bytes,
            truncated: false,
            stripper: AnsiStripper::default(),
            text: String::new(),
        })
    }

    pub fn output(&mut self, data: &str) {
        if self.truncated || data.is_empty() {
            return;
        }
        if self.bytes >= self.max_bytes {
            self.write_event("o", TRUNCATED_MARKER);
            self.truncated = true;
            return;
        }
        self.stripper.feed(data, &mut self.text);
        self.write_event("o", data);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if self.truncated || self.bytes >= self.max_bytes {
            return;
        }
        let dim = format!("{cols}x{rows}");
        self.write_event("r", &dim);
    }

    pub fn finish(mut self) -> String {
        self.finish_in_place()
    }

    pub fn finish_in_place(&mut self) -> String {
        if let Some(mut writer) = self.writer.take() {
            if let Err(e) = writer.flush() {
                tracing::warn!("failed to flush session recording: {e}");
            }
        }
        std::mem::take(&mut self.text)
    }

    fn write_event(&mut self, kind: &str, payload: &str) {
        let Some(ref mut writer) = self.writer else {
            return;
        };
        let elapsed = self.start.elapsed().as_secs_f64();
        let line = match serde_json::to_string(&(elapsed, kind, payload)) {
            Ok(mut s) => {
                s.push('\n');
                s
            }
            Err(e) => {
                tracing::warn!("failed to serialize recording event: {e}");
                self.writer = None;
                return;
            }
        };
        if let Err(e) = writer.write_all(line.as_bytes()) {
            tracing::warn!("failed to write recording event, disabling recorder: {e}");
            self.writer = None;
            return;
        }
        self.bytes = self.bytes.saturating_add(line.len() as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stripper_handles_ansi_osc_crlf_and_split_sequences() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("strip.cast");
        let mut rec = Recorder::create(&path, 80, 24, "web", 1024 * 1024).unwrap();

        // Colors, cursor movement, OSC title ended by BEL and by ST, CRLF and lone CR.
        rec.output("\x1b]0;user@web: ~\x07");
        rec.output("\x1b]2;another title\x1b\\");
        rec.output("\x1b[1;32muser@web\x1b[0m:\x1b[34m~\x1b[0m$ ");
        rec.output("systemctl\x1b[2Cstatus\r\n");
        rec.output("progress 10%\rprogress 100%\r\n");

        // Escape sequence split across two output() calls.
        rec.output("before \x1b[38;5;");
        rec.output("196mcolored\x1b[0m after\r\n");

        // Split OSC sequence ended by ST across two calls.
        rec.output("\x1b]0;split title\x1b");
        rec.output("\\done\t!\x07\x08");

        let text = rec.finish();
        assert_eq!(
            text,
            "user@web:~$ systemctlstatus\nprogress 10%\nprogress 100%\nbefore colored after\ndone\t!"
        );
    }

    #[test]
    fn recorder_writes_valid_cast_and_truncates_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("trunc.cast");
        // Allow ~1-2 small events before exceeding max_bytes.
        let mut rec = Recorder::create(&path, 120, 40, "web-prod", 50).unwrap();

        rec.output("first line\r\n");
        rec.output("second line pushing over cap\r\n");
        rec.output("third line triggers truncation\r\n");
        rec.output("fourth line must be dropped\r\n");
        let _ = rec.finish();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();

        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["version"], 2);
        assert_eq!(header["width"], 120);
        assert_eq!(header["height"], 40);
        assert_eq!(header["title"], "web-prod");

        let events: Vec<(f64, String, String)> = lines
            .map(|l| serde_json::from_str::<(f64, String, String)>(l).unwrap())
            .collect();

        assert!(events.iter().all(|(t, k, _)| *t >= 0.0 && k == "o"));
        let trunc_count = events
            .iter()
            .filter(|(_, _, d)| d == TRUNCATED_MARKER)
            .count();
        assert_eq!(trunc_count, 1, "expected exactly one truncation marker");
        assert_eq!(events.last().unwrap().2, TRUNCATED_MARKER);
        assert!(!events.iter().any(|(_, _, d)| d.contains("fourth line")));
    }
}
