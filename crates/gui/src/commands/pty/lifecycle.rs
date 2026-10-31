//! Session teardown: recording finalisation, reaping and the close commands.

use crate::commands::CommandError;
use crate::commands::{PtySession, PtyState};
use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

pub(super) fn finalize_db_session(
    db_session_id: Option<Uuid>,
    exit_code: i32,
    recorder: Option<bayesian_ssh::services::recording::SharedRecorder>,
) {
    let recorded_text = recorder.map(|rec_arc| match Arc::try_unwrap(rec_arc) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish(),
        Err(arc) => arc
            .lock()
            .map(|mut g| g.finish_in_place())
            .unwrap_or_default(),
    });

    let Some(session_id) = db_session_id else {
        return;
    };

    if let Ok(config) = AppConfig::load(None)
        && let Ok(db) = Database::new(&config)
    {
        if let Some(ref text) = recorded_text {
            let _ = db.index_session_text(session_id, text);
        }
        let _ = db.mark_session_terminated(&session_id.to_string(), exit_code);
        if config.recording.retention_days > 0
            && let Ok(paths) = db.prune_recordings(config.recording.retention_days)
        {
            for p in paths {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// Wait for a removed session's child (so it is not left as a zombie), record
/// its exit in the history database, then emit `history-updated` so the UI
/// reloads history only once the end is persisted. Blocks until the child
/// exits, so callers run it off the main thread.
pub(super) fn reap_session(session: PtySession, app: &AppHandle) {
    let PtySession {
        mut child,
        db_session_id,
        input,
        master,
        recorder,
        ..
    } = session;
    // Release our PTY ends first so the child sees the hangup before we block
    // (closing the input queue stops the writer thread, which drops the writer).
    drop(input);
    drop(master);
    let exit_code = child
        .wait()
        .map(|status| status.exit_code() as i32)
        .unwrap_or(-1);
    if db_session_id.is_some() || recorder.is_some() {
        finalize_db_session(db_session_id, exit_code, recorder);
        let _ = app.emit("history-updated", ());
    }
}

#[tauri::command]
pub fn count_active_sessions(state: State<'_, PtyState>) -> Result<usize, CommandError> {
    Ok(state.lock_sessions().len())
}

// `Child::kill` waits up to ~200 ms for the child to exit after SIGHUP: keep
// it off the main/UI thread.
#[tauri::command(async)]
pub fn close_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    close_window: Option<bool>,
) -> Result<(), CommandError> {
    let session = {
        let mut sessions = state.lock_sessions();
        sessions.remove(&session_id)
    };

    if let Some(mut session) = session {
        session.cancelled.store(true, Ordering::SeqCst);
        // Release a reader parked on flow control: the flag it re-checks is
        // set, and clearing the sink notifies it right away instead of after
        // the next 250 ms poll.
        session.sink.clear();

        if close_window.unwrap_or(true)
            && let Some(label) = session.popout_window.get()
            && let Some(window) = app.get_webview_window(&label)
        {
            let _ = window.close();
        }

        let _ = session.child.kill();
        let reap_app = app.clone();
        std::thread::spawn(move || reap_session(session, &reap_app));

        let _ = app.emit("session-closed", session_id);
    }

    Ok(())
}

#[tauri::command(async)]
pub fn close_all_ptys(app: AppHandle, state: State<'_, PtyState>) -> Result<usize, CommandError> {
    let session_ids: Vec<String> = {
        let sessions = state.lock_sessions();
        sessions.keys().cloned().collect()
    };

    let count = session_ids.len();
    for session_id in session_ids {
        let _ = close_pty(app.clone(), state.clone(), session_id, None);
    }

    Ok(count)
}
