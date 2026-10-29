use portable_pty::MasterPty;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Session;

use super::{
    get_db_and_config, DetachedSessionInfo, DockedSessionInfo, PopoutMainOverlap, PopoutOwner,
    PopoutSessionInfo, PtySession, PtyState, ReattachSessionInfo, MAIN_WINDOW_LABEL,
    MAX_DETACHED_BUFFER_BYTES, REPLAY_TRIM_SLACK_BYTES,
};

/// One batch of PTY output delivered on a session's output channel.
///
/// `bytes` is the UTF-8 byte length of `data`: the frontend acks exactly those
/// bytes once xterm has consumed the chunk, which is what releases the reader
/// thread's backpressure budget.
#[derive(Clone, Debug, Serialize)]
pub struct PtyChunk {
    pub data: String,
    pub bytes: usize,
}

/// Output the reader thread may hand to a channel before it parks and waits
/// for the frontend to catch up.
const OUTPUT_HIGH_WATER_BYTES: usize = 1024 * 1024;
/// Once acks bring the outstanding bytes down to this, the reader resumes.
const OUTPUT_LOW_WATER_BYTES: usize = 256 * 1024;
/// How often a parked reader wakes up to re-check for cancellation, so a
/// closed session can never leave it blocked forever.
const OUTPUT_WAIT_POLL: Duration = Duration::from_millis(250);

/// Byte counter behind the output channel's flow control.
///
/// The reader records what it handed to the frontend, `ack_pty` subtracts what
/// xterm has consumed, and the reader parks once the outstanding amount reaches
/// [`OUTPUT_HIGH_WATER_BYTES`], resuming when it is back at
/// [`OUTPUT_LOW_WATER_BYTES`]. Deliberately free of Tauri types so the
/// thresholds are unit-testable.
#[derive(Default)]
pub struct FlowControl {
    state: Mutex<FlowState>,
    wake: Condvar,
}

#[derive(Default)]
struct FlowState {
    /// Whether a sink is installed; without one nothing is counted or waited.
    attached: bool,
    /// Bytes sent on the sink that the frontend has not acked yet.
    unacked: usize,
}

impl FlowControl {
    fn lock(&self) -> std::sync::MutexGuard<'_, FlowState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A sink was installed (or replaced): the new consumer has not received
    /// any of the outstanding bytes, so the budget starts from zero again.
    fn attach(&self) {
        let mut state = self.lock();
        state.attached = true;
        state.unacked = 0;
        drop(state);
        self.wake.notify_all();
    }

    /// The sink went away (dock/detach/close/dead channel): stop counting and
    /// release a parked reader.
    fn detach(&self) {
        let mut state = self.lock();
        state.attached = false;
        state.unacked = 0;
        drop(state);
        self.wake.notify_all();
    }

    /// Account for a chunk handed to the sink. Without a sink nothing is
    /// counted, mirroring the fact that no wait can happen either.
    fn record(&self, bytes: usize) {
        let mut state = self.lock();
        if !state.attached {
            return;
        }
        state.unacked = state.unacked.saturating_add(bytes);
    }

    /// The frontend processed `bytes`: release that much budget.
    fn ack(&self, bytes: usize) {
        let mut state = self.lock();
        state.unacked = state.unacked.saturating_sub(bytes);
        drop(state);
        self.wake.notify_all();
    }

    #[cfg(test)]
    fn unacked(&self) -> usize {
        self.lock().unacked
    }

    /// Whether the consumer is far enough behind to stop the reader: only a
    /// full high-water mark triggers a pause, so a single chunk above the low
    /// mark still goes through.
    fn congested(&self) -> bool {
        let state = self.lock();
        state.attached && state.unacked >= OUTPUT_HIGH_WATER_BYTES
    }

    /// Park while the consumer is behind, re-checking [`OUTPUT_WAIT_POLL`] so a
    /// cancelled session or a cleared sink never blocks the reader forever.
    fn wait_while_congested(&self, stop: impl Fn() -> bool) {
        if !self.congested() {
            return;
        }
        loop {
            if stop() {
                return;
            }
            let state = self.lock();
            if !state.attached || state.unacked <= OUTPUT_LOW_WATER_BYTES {
                return;
            }
            // Nothing panics while the lock is held, so a poisoned mutex only
            // means "woken up": keep the guard and re-check the condition.
            let state = self
                .wake
                .wait_timeout(state, OUTPUT_WAIT_POLL)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            drop(state);
        }
    }
}

/// Per-session output sink: the channel currently attached (the main window or
/// one pop-out window) plus its flow-control counter.
///
/// Cloning is cheap and shares the same sink, so the reader thread and the
/// commands always see the same channel. `None` means nobody is listening:
/// output then only lands in the replay buffer and the recorder.
#[derive(Clone, Default)]
pub struct OutputSink(Arc<OutputSinkInner>);

#[derive(Default)]
struct OutputSinkInner {
    channel: Mutex<Option<Channel<PtyChunk>>>,
    flow: FlowControl,
}

impl OutputSink {
    pub fn new(channel: Channel<PtyChunk>) -> Self {
        let sink = Self::default();
        sink.set(channel);
        sink
    }

    fn lock_channel(&self) -> std::sync::MutexGuard<'_, Option<Channel<PtyChunk>>> {
        self.0
            .channel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Install (or replace) the channel.
    pub fn set(&self, channel: Channel<PtyChunk>) {
        *self.lock_channel() = Some(channel);
        self.0.flow.attach();
    }

    /// Stop delivering: the session was docked, detached, reopened in a pop-out
    /// window, closed, or its channel died. Returns the channel that was
    /// attached, so a caller whose handover failed can put it back.
    pub fn take(&self) -> Option<Channel<PtyChunk>> {
        let previous = self.lock_channel().take();
        self.0.flow.detach();
        previous
    }

    /// Stop delivering and drop the channel.
    pub fn clear(&self) {
        let _ = self.take();
    }

    /// Hand one chunk to the attached channel. No sink means the chunk only
    /// lives in the replay buffer.
    fn send(&self, data: &str) {
        // The channel mutex is held across the send so a concurrent `set`/
        // `clear` cannot make us drop the channel that replaced this one.
        let mut channel = self.lock_channel();
        let Some(sink) = channel.as_ref() else {
            return;
        };
        let chunk = PtyChunk {
            data: data.to_string(),
            bytes: data.len(),
        };
        // Count before sending: an ack for this chunk may arrive while the
        // send is still in flight, and it must not be lost under the counter.
        self.0.flow.record(chunk.bytes);
        if sink.send(chunk).is_err() {
            // The webview is gone (window closed or reloading): drop the sink
            // so the reader stops accounting against a dead channel.
            *channel = None;
            self.0.flow.detach();
        }
    }

    /// The frontend consumed `bytes`.
    fn ack(&self, bytes: usize) {
        self.0.flow.ack(bytes);
    }

    /// Park while the frontend is behind on this sink (see [`FlowControl`]).
    fn wait_while_congested(&self, stop: impl Fn() -> bool) {
        self.0.flow.wait_while_congested(stop);
    }
}

/// Exit status of a session's child, sent just before `pty-exit` so the UI can
/// tell ssh's own failures (exit 255) from a normal logout.
#[derive(Clone, Serialize)]
struct PtyExitStatus<'a> {
    session_id: &'a str,
    exit_code: i32,
}

struct WindowRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn window_outer_rect(window: &tauri::WebviewWindow) -> Result<WindowRect, String> {
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    Ok(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn rect_intersection_area(a: &WindowRect, b: &WindowRect) -> i64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    if right <= left || bottom <= top {
        return 0;
    }
    (right - left) as i64 * (bottom - top) as i64
}

fn rect_center_over(a: &WindowRect, container: &WindowRect) -> bool {
    let center_x = a.x + a.width as i32 / 2;
    let center_y = a.y + a.height as i32 / 2;
    center_x >= container.x
        && center_x <= container.x + container.width as i32
        && center_y >= container.y
        && center_y <= container.y + container.height as i32
}

/// Round a byte index down to the nearest UTF-8 char boundary so `String`
/// slicing never panics on multi-byte characters.
fn char_boundary_at_or_before(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn append_to_output_buffer(
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

fn take_full_replay(session: &PtySession) -> String {
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
fn lock_output_gate(session: &PtySession) -> std::sync::MutexGuard<'_, ()> {
    session
        .output_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the replay buffer with a frontend snapshot: the terminal the user
/// just left is now the whole history, and later output appends to it.
fn apply_snapshot(session: &PtySession, snapshot: Option<String>) {
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
fn switch_sink_on(session: &PtySession, output: Channel<PtyChunk>) -> String {
    let _gate = lock_output_gate(session);
    session.sink.set(output);
    take_full_replay(session)
}

/// Detach the current sink and (optionally) install the frontend snapshot of
/// what it had displayed, in one atomic step. Returns the detached channel so
/// a failed handover can restore it.
fn switch_sink_off(session: &PtySession, snapshot: Option<String>) -> Option<Channel<PtyChunk>> {
    let _gate = lock_output_gate(session);
    let previous = session.sink.take();
    apply_snapshot(session, snapshot);
    previous
}

fn seal_replay_offset(session: &PtySession) {
    if let (Ok(buffer), Ok(mut offset)) =
        (session.output_buffer.lock(), session.replay_offset.lock())
    {
        *offset = buffer.len();
    }
}

fn finalize_db_session(
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

    if let Ok(config) = AppConfig::load(None) {
        if let Ok(db) = Database::new(&config) {
            if let Some(ref text) = recorded_text {
                let _ = db.index_session_text(session_id, text);
            }
            let _ = db.mark_session_terminated(&session_id.to_string(), exit_code);
            if config.recording.retention_days > 0 {
                if let Ok(paths) = db.prune_recordings(config.recording.retention_days) {
                    for p in paths {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
    }
}

/// Wait for a removed session's child (so it is not left as a zombie), record
/// its exit in the history database, then emit `history-updated` so the UI
/// reloads history only once the end is persisted. Blocks until the child
/// exits, so callers run it off the main thread.
fn reap_session(session: PtySession, app: &AppHandle) {
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

// Opens the database and forks ssh: keep it off the main/UI thread.
#[tauri::command(async)]
pub fn spawn_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    connection_name: String,
    output: Channel<PtyChunk>,
) -> Result<(), String> {
    if state.lock_sessions().contains_key(&session_id) {
        // Overwriting the entry would drop the old PtySession without killing
        // its child, leaking a live ssh process and its reader thread.
        return Err(format!("PTY session '{session_id}' already exists"));
    }

    let (db, config) = get_db_and_config()?;

    let connection = db
        .get_connection(&connection_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{}' not found", connection_name))?;
    if connection.pending_change.is_some() {
        // The dialog must show the old and new route before ssh runs.
        return Err(bayesian_ssh::services::pending::desktop_error(&connection));
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
                return Err(format!("PTY session '{session_id}' already exists"));
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
            if let Some(rec) = &recorder_clone {
                if let Ok(mut g) = rec.lock() {
                    g.output(&str_data);
                }
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
) -> Result<(), String> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;
    session
        .input
        .send(data)
        .map_err(|_| format!("PTY session '{}' is no longer writable", session_id))
}

// Stays synchronous so the last resize always wins; TIOCSWINSZ is a
// non-blocking ioctl.
#[tauri::command]
pub fn resize_pty(
    state: State<'_, PtyState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
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
    if let Some(rec) = &session.recorder {
        if let Ok(mut g) = rec.lock() {
            g.resize(cols, rows);
        }
    }
    Ok(())
}

/// Force full-screen applications (vim, htop, less) to repaint: shrink the PTY
/// by one row, then restore the size ~60 ms later from a background thread.
/// The window-change signal makes them redraw everything, which repairs the
/// screen after a snapshot replay. Purely a display concern, so the recorder
/// is left alone.
#[tauri::command]
pub fn redraw_pty(state: State<'_, PtyState>, session_id: String) -> Result<(), String> {
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
        MasterPty::get_size(master.as_ref()).map_err(|e| e.to_string())?
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

#[tauri::command]
pub fn seal_session_ui(state: State<'_, PtyState>, session_id: String) -> Result<(), String> {
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
) -> Result<(), String> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is already detached", session_id));
    }

    session.detached.store(true, Ordering::SeqCst);
    session.popout_window.set(None);
    // Nobody listens while detached: output only accrues in the replay buffer,
    // seeded with what the frontend had on screen.
    let _ = switch_sink_off(session, snapshot);
    Ok(())
}

// Window creation from a synchronous command can deadlock (Tauri docs); run it
// off the main thread.
#[tauri::command(async)]
pub fn open_terminal_window(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    title: String,
    snapshot: Option<String>,
) -> Result<(), String> {
    let label = format!("terminal-{session_id}");

    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
        return Ok(());
    }

    let previous_sink = {
        let sessions = state.lock_sessions();
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;
        // The new window creates its own channel when it claims the session;
        // until then nobody listens, so stop delivering to this window.
        session.popout_window.set(Some(label.clone()));
        session.detached.store(false, Ordering::SeqCst);
        switch_sink_off(session, snapshot)
    };

    let window_title = format!("{title} — Bayesian SSH");
    let built = WebviewWindowBuilder::new(
        &app,
        label.clone(),
        WebviewUrl::App(format!("/terminal/{session_id}").into()),
    )
    .title(window_title)
    .inner_size(960.0, 640.0)
    .min_inner_size(480.0, 320.0)
    .decorations(false)
    .background_color(tauri::window::Color::from((12, 13, 18, 255)))
    .build();

    if let Err(e) = built {
        // No window will ever claim or dock this session: release the popout
        // binding and hand output back to the window that still displays it.
        if let Some(session) = state.lock_sessions().get(&session_id) {
            if session.popout_window.get().as_deref() == Some(label.as_str()) {
                session.popout_window.set(None);
                if let Some(channel) = previous_sink {
                    session.sink.set(channel);
                }
            }
        }
        return Err(e.to_string());
    }

    Ok(())
}

#[tauri::command]
pub fn claim_popout_session(
    state: State<'_, PtyState>,
    session_id: String,
    window_label: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, String> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

    match session.popout_window.get() {
        Some(existing) if existing != window_label => {
            return Err(format!(
                "PTY session '{session_id}' is attached to another window"
            ));
        }
        None => session.popout_window.set(Some(window_label)),
        _ => {}
    }

    session.detached.store(false, Ordering::SeqCst);
    let buffered_output = switch_sink_on(session, output);

    Ok(ReattachSessionInfo {
        session_id: session_id.clone(),
        connection_name: session.connection_name.clone(),
        buffered_output,
    })
}

/// Re-attach the main window to a session that was just docked from its
/// pop-out window. Sessions owned by a pop-out window or detached use
/// `claim_popout_session` / `reattach_pty` instead.
#[tauri::command]
pub fn attach_pty_output(
    state: State<'_, PtyState>,
    session_id: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, String> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

    if session.popout_window.get().is_some() {
        return Err(format!(
            "PTY session '{session_id}' is attached to a pop-out window"
        ));
    }
    if session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{session_id}' is detached"));
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
pub fn ack_pty(state: State<'_, PtyState>, session_id: String, bytes: usize) -> Result<(), String> {
    if let Some(session) = state.lock_sessions().get(&session_id) {
        session.sink.ack(bytes);
    }
    Ok(())
}

#[tauri::command]
pub fn list_detached_sessions(
    state: State<'_, PtyState>,
) -> Result<Vec<DetachedSessionInfo>, String> {
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
pub fn list_popout_sessions(state: State<'_, PtyState>) -> Result<Vec<PopoutSessionInfo>, String> {
    let sessions = state.lock_sessions();
    Ok(sessions
        .iter()
        .filter_map(|(session_id, session)| {
            session
                .popout_window
                .get()
                .map(|window_label| PopoutSessionInfo {
                    session_id: session_id.clone(),
                    connection_name: session.connection_name.clone(),
                    window_label,
                })
        })
        .collect())
}

#[tauri::command]
pub fn dock_popout_session(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    window_label: String,
    snapshot: Option<String>,
) -> Result<(), String> {
    let info = {
        let sessions = state.lock_sessions();
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

        if session.popout_window.get().as_deref() != Some(window_label.as_str()) {
            return Err(format!("PTY session '{session_id}' is not in this window"));
        }

        session.popout_window.set(None);
        session.detached.store(false, Ordering::SeqCst);
        // The main window re-attaches with `attach_pty_output` once it sees
        // `session-docked`; until then output only accrues in the replay
        // buffer, seeded with what the pop-out window had on screen.
        let _ = switch_sink_off(session, snapshot);

        DockedSessionInfo {
            session_id: session_id.clone(),
            connection_name: session.connection_name.clone(),
        }
    };

    // The main window owns the session again and replays it from its own
    // buffer: send it the identity only.
    let _ = app.emit("session-docked", info);

    if let Some(window) = app.get_webview_window(&window_label) {
        let _ = window.destroy();
    }

    Ok(())
}

#[tauri::command]
pub fn check_popout_main_overlap(
    app: AppHandle,
    popout_window_label: String,
) -> Result<PopoutMainOverlap, String> {
    const DOCK_OVERLAP_RATIO: f64 = 0.3;

    let popout = app
        .get_webview_window(&popout_window_label)
        .ok_or_else(|| format!("Window '{popout_window_label}' not found"))?;
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    let popout_rect = window_outer_rect(&popout)?;
    let main_rect = window_outer_rect(&main)?;
    let popout_area = popout_rect.width as i64 * popout_rect.height as i64;
    let overlap_area = rect_intersection_area(&popout_rect, &main_rect);
    let overlap_ratio = if popout_area > 0 {
        overlap_area as f64 / popout_area as f64
    } else {
        0.0
    };
    let center_over_main = rect_center_over(&popout_rect, &main_rect);
    let overlaps = overlap_area > 0;
    let should_dock = overlaps && (overlap_ratio >= DOCK_OVERLAP_RATIO || center_over_main);

    Ok(PopoutMainOverlap {
        overlaps,
        overlap_ratio,
        center_over_main,
        should_dock,
    })
}

#[tauri::command]
pub fn focus_terminal_window(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
) -> Result<(), String> {
    let label = {
        let sessions = state.lock_sessions();
        sessions
            .get(&session_id)
            .and_then(|session| session.popout_window.get())
            .ok_or_else(|| format!("PTY session '{session_id}' is not popped out"))?
    };

    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("Window '{label}' not found"))?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reattach_pty(
    state: State<'_, PtyState>,
    session_id: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, String> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if !session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is not detached", session_id));
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

#[tauri::command]
pub fn count_active_sessions(state: State<'_, PtyState>) -> Result<usize, String> {
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
) -> Result<(), String> {
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

        if close_window.unwrap_or(true) {
            if let Some(label) = session.popout_window.get() {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.close();
                }
            }
        }

        let _ = session.child.kill();
        let reap_app = app.clone();
        std::thread::spawn(move || reap_session(session, &reap_app));

        let _ = app.emit("session-closed", session_id);
    }

    Ok(())
}

#[tauri::command(async)]
pub fn close_all_ptys(app: AppHandle, state: State<'_, PtyState>) -> Result<usize, String> {
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

/// Read the PRIMARY selection on Linux, where middle-click paste uses it.
#[cfg(target_os = "linux")]
fn primary_selection() -> Option<String> {
    use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind};

    let mut clipboard = Clipboard::new().ok()?;
    clipboard
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .ok()
}

/// Only X11 and Wayland have a PRIMARY selection; elsewhere middle-click paste
/// falls back to the default clipboard in the frontend.
#[cfg(not(target_os = "linux"))]
fn primary_selection() -> Option<String> {
    None
}

/// Read the PRIMARY selection, the X11/Wayland buffer middle-click paste uses.
///
/// Runs off the main thread because a selection owner that does not answer can
/// make the clipboard backend block for a while. `None` when no owner answers
/// or the backend is unavailable (headless session, no display server).
#[tauri::command(async)]
pub fn read_primary_selection() -> Option<String> {
    primary_selection()
}

#[cfg(test)]
mod tests {
    use super::{FlowControl, OUTPUT_HIGH_WATER_BYTES, OUTPUT_LOW_WATER_BYTES, OUTPUT_WAIT_POLL};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
    use std::sync::Arc;
    use std::time::Duration;

    /// Start a waiter thread and signal when `wait_while_congested` returns.
    fn spawn_waiter(
        flow: &Arc<FlowControl>,
        stop: impl Fn() -> bool + Send + 'static,
    ) -> Receiver<()> {
        let (tx, rx) = channel();
        let flow = Arc::clone(flow);
        std::thread::spawn(move || {
            flow.wait_while_congested(stop);
            let _ = tx.send(());
        });
        rx
    }

    /// Waiting longer than one poll interval without a signal proves the
    /// reader is really parked.
    fn assert_still_parked(rx: &Receiver<()>) {
        assert_eq!(
            rx.recv_timeout(OUTPUT_WAIT_POLL + Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout),
            "reader resumed before the frontend caught up"
        );
    }

    fn assert_released(rx: &Receiver<()>) {
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(()),
            "reader stayed parked"
        );
    }

    #[test]
    fn reader_parks_at_high_water_and_resumes_at_low_water() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        flow.record(OUTPUT_HIGH_WATER_BYTES);
        let waiter = spawn_waiter(&flow, || false);
        assert_still_parked(&waiter);

        // One byte above the low-water mark is still too much.
        flow.ack(OUTPUT_HIGH_WATER_BYTES - OUTPUT_LOW_WATER_BYTES - 1);
        assert_eq!(flow.unacked(), OUTPUT_LOW_WATER_BYTES + 1);
        assert_still_parked(&waiter);

        flow.ack(1);
        assert_eq!(flow.unacked(), OUTPUT_LOW_WATER_BYTES);
        assert_released(&waiter);
    }

    #[test]
    fn reader_only_parks_once_the_high_water_mark_is_reached() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        // One byte short of the high-water mark: deliver it and keep going.
        flow.record(OUTPUT_HIGH_WATER_BYTES - 1);
        let waiter = spawn_waiter(&flow, || false);
        assert_released(&waiter);

        flow.record(1);
        let waiter = spawn_waiter(&flow, || false);
        assert_still_parked(&waiter);
        flow.ack(usize::MAX);
        assert_released(&waiter);
    }

    #[test]
    fn nothing_is_counted_or_waited_for_without_a_sink() {
        let flow = Arc::new(FlowControl::default());
        flow.record(OUTPUT_HIGH_WATER_BYTES * 4);
        // No sink attached: output is nobody's problem, so it is neither
        // counted nor waited for.
        assert_eq!(flow.unacked(), 0);
        let waiter = spawn_waiter(&flow, || false);
        assert_released(&waiter);
    }

    #[test]
    fn replacing_or_clearing_the_sink_resets_and_releases() {
        for clear in [true, false] {
            let flow = Arc::new(FlowControl::default());
            flow.attach();
            flow.record(OUTPUT_HIGH_WATER_BYTES);
            let waiter = spawn_waiter(&flow, || false);
            assert_still_parked(&waiter);

            // The budget belongs to the consumer that left.
            if clear {
                flow.detach();
            } else {
                flow.attach();
            }
            assert_eq!(flow.unacked(), 0);
            assert_released(&waiter);
        }
    }

    #[test]
    fn ack_saturates_instead_of_underflowing() {
        let flow = FlowControl::default();
        flow.attach();
        flow.record(OUTPUT_LOW_WATER_BYTES);
        // A late or duplicated ack must not wrap the counter into a huge value
        // that parks the reader forever.
        flow.ack(usize::MAX);
        assert_eq!(flow.unacked(), 0);
        flow.ack(64);
        assert_eq!(flow.unacked(), 0);
    }

    #[test]
    fn cancelled_reader_leaves_the_wait() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        flow.record(OUTPUT_HIGH_WATER_BYTES);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let waiter = spawn_waiter(&flow, move || flag.load(Ordering::SeqCst));
        assert_still_parked(&waiter);

        // Polled, no notify needed: close_pty may only flip the flag.
        cancelled.store(true, Ordering::SeqCst);
        assert_released(&waiter);
    }
}
