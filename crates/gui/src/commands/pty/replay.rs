//! Detached-output replay buffer helpers.

use super::output::PtyChunk;
use crate::commands::{PtySession, MAX_DETACHED_BUFFER_BYTES, REPLAY_TRIM_SLACK_BYTES};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

/// Round a byte index down to the nearest UTF-8 char boundary so `String`
/// slicing never panics on multi-byte characters.
pub(super) fn char_boundary_at_or_before(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(super) fn append_to_output_buffer(
    buffer: &Arc<Mutex<String>>,
    replay_offset: &Arc<Mutex<usize>>,
    data: &str,
) {
    let Ok(mut buffer) = buffer.lock() else {
        return;
    };
    buffer.push_str(data);
    // Trim in REPLAY_TRIM_SLACK_BYTES steps: draining the front of a 512 KiB
    // string on every chunk once the buffer is full is an O(n) memmove per
    // output event.
    if buffer.len() > MAX_DETACHED_BUFFER_BYTES + REPLAY_TRIM_SLACK_BYTES {
        // `String::drain` panics if the range cuts a multi-byte UTF-8 char —
        // clamp to a char boundary first. Without this, a session printing
        // non-ASCII output would panic the reader thread and freeze the
        // session silently.
        let overflow = buffer.len() - MAX_DETACHED_BUFFER_BYTES;
        let cut = char_boundary_at_or_before(&buffer, overflow);
        buffer.drain(..cut);
        if let Ok(mut offset) = replay_offset.lock() {
            *offset = offset.saturating_sub(cut);
        }
    }
}

pub(super) fn take_full_replay(session: &PtySession) -> String {
    let buffer = session
        .output_buffer
        .lock()
        .map(|b| b.clone())
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

/// Replace the replay buffer with a frontend snapshot: the terminal the user
/// just left is now the whole history, and later output appends to it.
pub(super) fn apply_snapshot(session: &PtySession, snapshot: Option<String>) {
    let Some(snapshot) = snapshot else {
        return;
    };
    if let Ok(mut buffer) = session.output_buffer.lock() {
        *buffer = snapshot;
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
