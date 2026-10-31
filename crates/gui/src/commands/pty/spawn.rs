//! Spawning a PTY and the commands that drive its input side.

use super::lifecycle::reap_session;
use super::output::{OutputSink, PtyChunk};
use super::replay::append_to_output_buffer;
use crate::commands::CommandError;
use crate::commands::{open_db_and_config, PopoutOwner, PtySession, PtyState, MAIN_WINDOW_LABEL};
use bayesian_ssh::models::Session;
use portable_pty::MasterPty;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Exit status of a session's child, sent just before `pty-exit` so the UI can
/// tell ssh's own failures (exit 255) from a normal logout.
#[derive(Clone, Serialize)]
struct PtyExitStatus<'a> {
    session_id: &'a str,
    exit_code: i32,
}

// Opens the database and forks ssh: keep it off the main/UI thread.
#[tauri::command(async)]
pub fn spawn_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    connection_name: String,
    output: Channel<PtyChunk>,
) -> Result<(), CommandError> {
    if state.lock_sessions().contains_key(&session_id) {
        // Overwriting the entry would drop the old PtySession without killing
        // its child, leaking a live ssh process and its reader thread.
        return Err(format!("PTY session '{session_id}' already exists").into());
    }

    let (db, config) = open_db_and_config()?;

    let connection = db
        .connection(&connection_name)?
        .ok_or_else(|| format!("Connection '{}' not found", connection_name))?;
    if connection.pending_change.is_some() {
        // The dialog must show the old and new route before ssh runs.
        return Err(bayesian_ssh::services::pending::desktop_error(&connection).into());
    }

    // Record last used
    let mut updated_conn = connection.clone();
    updated_conn.update_last_used();
    let _ = db.touch_connection(&updated_conn);

    // Build the SSH command arguments. The configured host-key policy covers
    // the bastion hop; the target's prompt stays the user's ssh's.
    let argv = bayesian_ssh::services::transport::SubprocessTransport::build_shell_argv(
        &connection,
        config.transport.host_key_policy(),
    );
    let (cmd_name, args) = argv
        .split_first()
        .ok_or_else(|| "SSH command generation failed".to_string())?;

    // Spawn inside a fresh PTY via the shared module. It inherits env vars so
    // Kerberos tickets (KRB5CCNAME) and ssh-agent (SSH_AUTH_SOCK) are passed
    // down, and forces TERM=xterm-256color for xterm.js compatibility.
    let spawned =
        bayesian_ssh::services::pty::spawn(bayesian_ssh::services::pty::PtySpawnOptions {
            cmd_name,
            args,
            rows: bayesian_ssh::services::pty::DEFAULT_ROWS,
            cols: bayesian_ssh::services::pty::DEFAULT_COLS,
            agent_socket: config.auth.agent_socket.as_deref(),
        })?;

    let child = spawned.child;

    let want_record =
        bayesian_ssh::services::recording::should_record(&connection, &config.recording, None);
    let mut db_session_id: Option<Uuid> = None;
    let mut recorder: Option<bayesian_ssh::services::recording::SharedRecorder> = None;
    if config.auto_save_history || want_record {
        let mut session = Session::new(connection.clone());
        session.transport = Some("subprocess".to_string());
        if want_record {
            let path = bayesian_ssh::services::recording::recording_path(&config, session.id);
            match bayesian_ssh::services::recording::Recorder::create(
                &path,
                bayesian_ssh::services::pty::DEFAULT_COLS,
                bayesian_ssh::services::pty::DEFAULT_ROWS,
                &connection.name,
                config.recording.max_bytes_per_session,
            ) {
                Ok(rec) => {
                    session.recording_path = Some(path.to_string_lossy().to_string());
                    recorder = Some(Arc::new(Mutex::new(rec)));
                }
                Err(e) => {
                    eprintln!("Failed to start session recorder: {e}");
                    session.recording_path = None;
                }
            }
        }
        session.context = bayesian_ssh::services::context::capture_without_cwd(&db);
        if let Err(e) = db.add_session(&session) {
            eprintln!("Failed to record session start: {e}");
            recorder = None;
        } else {
            db_session_id = Some(session.id);
            if let Some(pid) = child.process_id() {
                let mut active_session = session;
                active_session.mark_active(pid);
                let _ = db.update_session(&active_session);
            }
        }
    }

    let reader = spawned.reader;
    let mut writer = spawned.writer;

    // Writer thread: drains write_pty input in order, so a child that stops
    // reading its terminal can never block the IPC thread.
    let (input, input_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for data in input_rx {
            if bayesian_ssh::services::pty::write_all(writer.as_mut(), &data).is_err() {
                break;
            }
        }
    });

    // Cancelled flag: set by close_pty so the reader thread won't emit pty-exit
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);
    let detached = Arc::new(AtomicBool::new(false));
    let output_buffer = Arc::new(Mutex::new(String::new()));
    let output_buffer_clone = Arc::clone(&output_buffer);
    let replay_offset = Arc::new(Mutex::new(0));
    let replay_offset_clone = Arc::clone(&replay_offset);
    let popout_window = PopoutOwner::default();
    let popout_window_clone = popout_window.clone();
    let recorder_clone = recorder.clone();
    // Sink installed before the reader thread starts, so the first chunk of
    // output cannot be missed.
    let sink = OutputSink::new(output);
    let sink_clone = sink.clone();
    let output_gate = Arc::new(Mutex::new(()));
    let output_gate_clone = Arc::clone(&output_gate);

    let session = PtySession {
        input,
        child,
        master: Arc::new(Mutex::new(spawned.master)),
        cancelled,
        db_session_id,
        connection_name: connection.name.clone(),
        detached,
        output_buffer,
        replay_offset,
        output_gate,
        sink,
        popout_window,
        recorder,
    };

    // Store in global state — the master keeps the PTY fd alive for the
    // session duration. Re-check the id under the lock: commands run
    // concurrently, so another spawn may have claimed it meanwhile.
    {
        let mut sessions = state.lock_sessions();
        match sessions.entry(session_id.clone()) {
            Entry::Vacant(slot) => {
                slot.insert(session);
            }
            Entry::Occupied(_) => {
                drop(sessions);
                let mut session = session;
                session.cancelled.store(true, Ordering::SeqCst);
                let _ = session.child.kill();
                let app = app.clone();
                std::thread::spawn(move || reap_session(session, &app));
                return Err(format!("PTY session '{session_id}' already exists").into());
            }
        }
    }

    // Start background thread to read from PTY master and emit to frontend
    let session_id_clone = session_id.clone();
    let app_handle = app.clone();
    let sessions_map = Arc::clone(&state.sessions);

    std::thread::spawn(move || {
        let mut reader = reader;

        bayesian_ssh::services::pty::read_loop(&mut reader, |str_data| {
            if let Some(rec) = &recorder_clone
                && let Ok(mut g) = rec.lock()
            {
                g.output(&str_data);
            }

            // Replay append and live delivery happen under one gate, so a
            // command that swaps the sink (dock / re-attach / detach) can
            // never replay a chunk that was also delivered on the channel.
            {
                let _gate = output_gate_clone
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                append_to_output_buffer(&output_buffer_clone, &replay_offset_clone, &str_data);
                sink_clone.send(&str_data);
            }

            // The frontend is behind: park until it acks down to the low-water
            // mark (or the session is closed / the sink is cleared). No lock
            // is held here, and the wait re-checks cancellation every 250 ms.
            sink_clone.wait_while_congested(|| cancelled_clone.load(Ordering::SeqCst));
        });
        drop(recorder_clone);

        // Only emit pty-exit if this was NOT a manual close (avoids ghost events)
        if !cancelled_clone.load(Ordering::SeqCst) {
            // The remote side ended the session. The frontend only tears down
            // its UI on pty-exit and never calls close_pty, so release the
            // PTY, the replay buffer and the child process here. The pointer
            // check guards against removing a newer session reusing the id.
            let mut finished = {
                let mut sessions = sessions_map
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let owned = sessions
                    .get(&session_id_clone)
                    .is_some_and(|session| Arc::ptr_eq(&session.cancelled, &cancelled_clone));
                if owned {
                    sessions.remove(&session_id_clone)
                } else {
                    None
                }
            };

            // The read loop ended, so the child has exited or is about to;
            // `reap_session` would wait for it anyway and the status is cached.
            if let Some(session) = finished.as_mut() {
                let exit_code = session
                    .child
                    .wait()
                    .map(|status| status.exit_code() as i32)
                    .unwrap_or(-1);
                let _ = app_handle.emit_to(
                    MAIN_WINDOW_LABEL,
                    "pty-exit-status",
                    PtyExitStatus {
                        session_id: &session_id_clone,
                        exit_code,
                    },
                );
            }

            // A pop-out window shows the disconnect via `pty-exit:{id}`; the
            // main window tracks every session's exit via `pty-exit`.
            if let Some(label) = popout_window_clone.get() {
                let _ = app_handle.emit_to(&label, &format!("pty-exit:{session_id_clone}"), ());
            }
            let _ = app_handle.emit_to(MAIN_WINDOW_LABEL, "pty-exit", &session_id_clone);

            if let Some(session) = finished {
                reap_session(session, &app_handle);
                let _ = app_handle.emit("session-closed", session_id_clone);
            }
        }
    });

    Ok(())
}

// Stays synchronous so keystrokes are queued in IPC order (async commands run
// concurrently and could reorder them); the queue never blocks.
#[tauri::command]
pub fn write_pty(
    state: State<'_, PtyState>,
    session_id: String,
    data: String,
) -> Result<(), CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;
    session.input.send(data).map_err(|_| {
        CommandError::from(format!(
            "PTY session '{}' is no longer writable",
            session_id
        ))
    })
}

// Stays synchronous so the last resize always wins; TIOCSWINSZ is a
// non-blocking ioctl.
#[tauri::command]
pub fn resize_pty(
    state: State<'_, PtyState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;
    let master = session
        .master
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    bayesian_ssh::services::pty::resize(master.as_ref(), cols, rows)?;
    drop(master);
    if let Some(rec) = &session.recorder
        && let Ok(mut g) = rec.lock()
    {
        g.resize(cols, rows);
    }
    Ok(())
}

/// Force full-screen applications (vim, htop, less) to repaint: shrink the PTY
/// by one row, then restore the size ~60 ms later from a background thread.
/// The window-change signal makes them redraw everything, which repairs the
/// screen after a snapshot replay. Purely a display concern, so the recorder
/// is left alone.
#[tauri::command]
pub fn redraw_pty(state: State<'_, PtyState>, session_id: String) -> Result<(), CommandError> {
    let master = {
        let sessions = state.lock_sessions();
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;
        Arc::clone(&session.master)
    };

    let size = {
        let master = master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        MasterPty::get_size(master.as_ref())?
    };

    // One row less is a real size change, so the application gets SIGWINCH and
    // repaints; the original size comes back once it had time to do so.
    {
        let master = master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bayesian_ssh::services::pty::resize(
            master.as_ref(),
            size.cols,
            size.rows.saturating_sub(1).max(1),
        )?;
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        let master = master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = bayesian_ssh::services::pty::resize(master.as_ref(), size.cols, size.rows);
    });

    Ok(())
}
