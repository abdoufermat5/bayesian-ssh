//! Detaching, reattaching and acknowledging output of live sessions.

use super::output::PtyChunk;
use super::replay::{seal_replay_offset, switch_sink_off, switch_sink_on};
use crate::commands::CommandError;
use crate::commands::{DetachedSessionInfo, PtyState, ReattachSessionInfo};
use std::sync::atomic::Ordering;
use tauri::State;
use tauri::ipc::Channel;

#[tauri::command]
pub fn seal_session_ui(state: State<'_, PtyState>, session_id: String) -> Result<(), CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;
    seal_replay_offset(session);
    Ok(())
}

#[tauri::command]
pub fn detach_pty(
    state: State<'_, PtyState>,
    session_id: String,
    snapshot: Option<String>,
) -> Result<(), CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is already detached", session_id).into());
    }

    session.detached.store(true, Ordering::SeqCst);
    session.popout_window.set(None);
    // Nobody listens while detached: output only accrues in the replay buffer,
    // seeded with what the frontend had on screen.
    let _ = switch_sink_off(session, snapshot);
    Ok(())
}

/// Re-attach the main window to a session that was just docked from its
/// pop-out window. Sessions owned by a pop-out window or detached use
/// `claim_popout_session` / `reattach_pty` instead.
#[tauri::command]
pub fn attach_pty_output(
    state: State<'_, PtyState>,
    session_id: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

    if session.popout_window.get().is_some() {
        return Err(format!("PTY session '{session_id}' is attached to a pop-out window").into());
    }
    if session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{session_id}' is detached").into());
    }

    let buffered_output = switch_sink_on(session, output);

    Ok(ReattachSessionInfo {
        session_id: session_id.clone(),
        connection_name: session.connection_name.clone(),
        buffered_output,
    })
}

/// The frontend has written (or dropped) `bytes` of output. Acks race with
/// session teardown by design, so an unknown session is not an error.
#[tauri::command]
pub fn ack_pty(
    state: State<'_, PtyState>,
    session_id: String,
    bytes: usize,
) -> Result<(), CommandError> {
    if let Some(session) = state.lock_sessions().get(&session_id) {
        session.sink.ack(bytes);
    }
    Ok(())
}

#[tauri::command]
pub fn list_detached_sessions(
    state: State<'_, PtyState>,
) -> Result<Vec<DetachedSessionInfo>, CommandError> {
    let sessions = state.lock_sessions();
    Ok(sessions
        .iter()
        .filter(|(_, session)| {
            session.detached.load(Ordering::SeqCst) && session.popout_window.is_none()
        })
        .map(|(session_id, session)| DetachedSessionInfo {
            session_id: session_id.clone(),
            connection_name: session.connection_name.clone(),
        })
        .collect())
}

#[tauri::command]
pub fn reattach_pty(
    state: State<'_, PtyState>,
    session_id: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if !session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is not detached", session_id).into());
    }

    session.popout_window.set(None);
    session.detached.store(false, Ordering::SeqCst);
    let buffered_output = switch_sink_on(session, output);

    Ok(ReattachSessionInfo {
        session_id: session_id.clone(),
        connection_name: session.connection_name.clone(),
        buffered_output,
    })
}
