//! Detached-output replay buffer helpers.

use super::output::PtyChunk;
use crate::commands::{MAX_DETACHED_BUFFER_BYTES, PtySession, REPLAY_TRIM_SLACK_BYTES};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

/// Chunked ring buffer for detached PTY replay.
///
/// Pushing a chunk and trimming whole chunks from the front are both O(1),
/// avoiding O(n) memmoves across the 512 KiB replay window on every trim.
#[derive(Default)]
pub struct ReplayBuffer {
    chunks: VecDeque<String>,
    total_bytes: usize,
}

impl ReplayBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.total_bytes
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.total_bytes == 0
    }

    /// Push a chunk and trim from the front once the buffer exceeds
    /// `MAX_DETACHED_BUFFER_BYTES + REPLAY_TRIM_SLACK_BYTES`.
    /// Returns the number of bytes dropped from the front.
    pub fn push(&mut self, data: &str) -> usize {
        if data.is_empty() {
            return 0;
        }
        self.total_bytes += data.len();
        self.chunks.push_back(data.to_string());

        let mut dropped = 0usize;
        if self.total_bytes > MAX_DETACHED_BUFFER_BYTES + REPLAY_TRIM_SLACK_BYTES {
            while self.total_bytes > MAX_DETACHED_BUFFER_BYTES {
                if self.chunks.len() > 1 {
                    let Some(front) = self.chunks.pop_front() else {
                        break;
                    };
                    let front_len = front.len();
                    self.total_bytes -= front_len;
                    dropped += front_len;
                } else if let Some(front) = self.chunks.front_mut() {
                    // Single chunk exceeds MAX_DETACHED_BUFFER_BYTES: split at
                    // a valid UTF-8 char boundary so slicing never panics.
                    let overflow = front.len() - MAX_DETACHED_BUFFER_BYTES;
                    let mut cut = overflow;
                    while cut < front.len() && !front.is_char_boundary(cut) {
                        cut += 1;
                    }
                    front.drain(..cut);
                    self.total_bytes -= cut;
                    dropped += cut;
                    break;
                } else {
                    break;
                }
            }
        }
        dropped
    }

    /// Replace all buffered content with a single snapshot string.
    pub fn replace(&mut self, snapshot: String) {
        self.chunks.clear();
        self.total_bytes = snapshot.len();
        if !snapshot.is_empty() {
            self.chunks.push_back(snapshot);
        }
    }

    /// Concatenate all chunks into a contiguous replay string.
    pub fn flatten(&self) -> String {
        let mut out = String::with_capacity(self.total_bytes);
        for chunk in &self.chunks {
            out.push_str(chunk);
        }
        out
    }
}

pub(super) fn append_to_output_buffer(
    buffer: &Arc<Mutex<ReplayBuffer>>,
    replay_offset: &Arc<Mutex<usize>>,
    data: &str,
) {
    let Ok(mut buffer) = buffer.lock() else {
        return;
    };
    let dropped = buffer.push(data);
    if dropped > 0
        && let Ok(mut offset) = replay_offset.lock()
    {
        *offset = offset.saturating_sub(dropped);
    }
}

pub(super) fn take_full_replay(session: &PtySession) -> String {
    let buffer = session
        .output_buffer
        .lock()
        .map(|b| b.flatten())
        .unwrap_or_default();
    if let Ok(mut offset) = session.replay_offset.lock() {
        *offset = buffer.len();
    }
    buffer
}

/// Lock the gate that serialises the reader thread's "append to the replay
/// buffer + deliver" step against commands swapping the sink or reading the
/// replay.
pub(super) fn lock_output_gate(session: &PtySession) -> std::sync::MutexGuard<'_, ()> {
    session
        .output_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keep at most `max_bytes` from the tail of `s`, aligned to a UTF-8 char
/// boundary so slicing never panics on multi-byte characters.
fn truncate_snapshot_tail(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = s.len() - max_bytes;
    while cut < s.len() && !s.is_char_boundary(cut) {
        cut += 1;
    }
    s.drain(..cut);
    s
}

/// Replace the replay buffer with a frontend snapshot: the terminal the user
/// just left is now the whole history, and later output appends to it.
pub(super) fn apply_snapshot(session: &PtySession, snapshot: Option<String>) {
    let Some(snapshot) = snapshot else {
        return;
    };
    let capped = truncate_snapshot_tail(snapshot, MAX_DETACHED_BUFFER_BYTES);
    if let Ok(mut buffer) = session.output_buffer.lock() {
        buffer.replace(capped);
    }
    if let Ok(mut offset) = session.replay_offset.lock() {
        *offset = 0;
    }
}

/// Attach a new sink and read the replay in one atomic step: a chunk is either
/// still in the buffer (and replayed) or delivered live, never both and never
/// neither.
pub(super) fn switch_sink_on(session: &PtySession, output: Channel<PtyChunk>) -> String {
    let _gate = lock_output_gate(session);
    session.sink.set(output);
    take_full_replay(session)
}

/// Detach the current sink and (optionally) install the frontend snapshot of
/// what it had displayed, in one atomic step. Returns the detached channel so
/// a failed handover can restore it.
pub(super) fn switch_sink_off(
    session: &PtySession,
    snapshot: Option<String>,
) -> Option<Channel<PtyChunk>> {
    let _gate = lock_output_gate(session);
    let previous = session.sink.take();
    apply_snapshot(session, snapshot);
    previous
}

pub(super) fn seal_replay_offset(session: &PtySession) {
    if let (Ok(buffer), Ok(mut offset)) =
        (session.output_buffer.lock(), session.replay_offset.lock())
    {
        *offset = buffer.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_tail_truncates_at_utf8_char_boundary() {
        let small = "hello".to_string();
        assert_eq!(truncate_snapshot_tail(small, 10), "hello");

        // Each '€' is 3 bytes. 5 chars = 15 bytes; capping to 7 bytes must keep
        // the last 2 full chars (6 bytes) without panicking.
        let utf8 = "€€€€€".to_string();
        let capped = truncate_snapshot_tail(utf8, 7);
        assert_eq!(capped, "€€");
        assert!(capped.len() <= 7);

        let oversized = "x".repeat(MAX_DETACHED_BUFFER_BYTES + 1024);
        let capped = truncate_snapshot_tail(oversized, MAX_DETACHED_BUFFER_BYTES);
        assert_eq!(capped.len(), MAX_DETACHED_BUFFER_BYTES);
    }

    #[test]
    fn replay_buffer_trims_whole_chunks_in_o1_and_updates_offset() {
        let buffer = Arc::new(Mutex::new(ReplayBuffer::new()));
        let offset = Arc::new(Mutex::new(100_000usize));

        // Push 4 KiB chunks so the final push crosses MAX + SLACK and triggers a trim.
        let chunk = "a".repeat(4096);
        let count = (MAX_DETACHED_BUFFER_BYTES + REPLAY_TRIM_SLACK_BYTES) / 4096 + 1;
        for _ in 0..count {
            append_to_output_buffer(&buffer, &offset, &chunk);
        }

        let guard = buffer.lock().unwrap();
        assert!(guard.len() <= MAX_DETACHED_BUFFER_BYTES);
        assert_eq!(guard.flatten().len(), guard.len());
        assert!(*offset.lock().unwrap() < 100_000);
    }

    #[test]
    fn replay_buffer_splits_single_oversized_utf8_chunk_safely() {
        let mut buf = ReplayBuffer::new();
        // '€' is 3 bytes; build a single chunk exceeding MAX + SLACK.
        let repeats = (MAX_DETACHED_BUFFER_BYTES + REPLAY_TRIM_SLACK_BYTES) / 3 + 10;
        let big = "€".repeat(repeats);
        let dropped = buf.push(&big);

        assert!(dropped > 0);
        assert!(buf.len() <= MAX_DETACHED_BUFFER_BYTES);
        assert_eq!(buf.len() % 3, 0);
        assert_eq!(buf.flatten().len(), buf.len());
    }

    #[test]
    fn replay_buffer_replace_and_flatten() {
        let mut buf = ReplayBuffer::new();
        assert!(buf.is_empty());
        buf.push("hello ");
        buf.push("world");
        assert_eq!(buf.flatten(), "hello world");

        buf.replace("snapshot".to_string());
        assert_eq!(buf.len(), 8);
        assert_eq!(buf.flatten(), "snapshot");

        buf.replace(String::new());
        assert!(buf.is_empty());
        assert_eq!(buf.flatten(), "");
    }
}
