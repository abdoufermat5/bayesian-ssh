use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::Session;

use super::{
    get_db_and_config, DetachedSessionInfo, PopoutMainOverlap, PopoutSessionInfo, PtySession,
    PtyState, ReattachSessionInfo, MAX_DETACHED_BUFFER_BYTES,
};

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

fn append_to_output_buffer(
    buffer: &Arc<Mutex<String>>,
    replay_offset: &Arc<Mutex<usize>>,
    data: &str,
) {
    let Ok(mut buffer) = buffer.lock() else {
        return;
    };
    buffer.push_str(data);
    if buffer.len() > MAX_DETACHED_BUFFER_BYTES {
        let overflow = buffer.len() - MAX_DETACHED_BUFFER_BYTES;
        buffer.drain(..overflow);
        if let Ok(mut offset) = replay_offset.lock() {
            *offset = offset.saturating_sub(overflow);
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

fn seal_replay_offset(session: &PtySession) {
    if let (Ok(buffer), Ok(mut offset)) =
        (session.output_buffer.lock(), session.replay_offset.lock())
    {
        *offset = buffer.len();
    }
}

fn finalize_db_session(db_session_id: Option<Uuid>, exit_code: i32) {
    let Some(session_id) = db_session_id else {
        return;
    };

    if let Ok(config) = AppConfig::load(None) {
        if let Ok(db) = Database::new(&config) {
            let _ = db.mark_session_terminated(&session_id.to_string(), exit_code);
        }
    }
}

#[tauri::command]
pub fn spawn_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    connection_name: String,
) -> Result<(), String> {
    let (db, config) = get_db_and_config()?;

    let connection = db
        .get_connection(&connection_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{}' not found", connection_name))?;

    // Record last used
    let mut updated_conn = connection.clone();
    updated_conn.update_last_used();
    let _ = db.update_connection(&updated_conn);

    // Build the SSH command arguments
    let pty_system = native_pty_system();

    let argv =
        bayesian_ssh::services::transport::SubprocessTransport::build_shell_argv(&connection);
    let (cmd_name, args) = argv
        .split_first()
        .ok_or_else(|| "SSH command generation failed".to_string())?;

    let mut cmd_builder = CommandBuilder::new(cmd_name);
    cmd_builder.args(args);

    // Inherit env vars so Kerberos tickets (KRB5CCNAME) and ssh-agent (SSH_AUTH_SOCK) are passed down
    for (key, val) in std::env::vars() {
        cmd_builder.env(key, val);
    }

    // Open PTY Pair
    let pty_pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pty_pair
        .slave
        .spawn_command(cmd_builder)
        .map_err(|e| e.to_string())?;

    let mut db_session_id: Option<Uuid> = None;
    if config.auto_save_history {
        let mut session = Session::new(connection.clone());
        session.transport = Some("subprocess".to_string());
        if let Err(e) = db.add_session(&session) {
            eprintln!("Failed to record session start: {e}");
        } else {
            db_session_id = Some(session.id);
            if let Some(pid) = child.process_id() {
                let mut active_session = session;
                active_session.mark_active(pid);
                let _ = db.update_session(&active_session);
            }
        }
    }

    let reader = pty_pair
        .master
        .try_clone_reader()
        .map_err(|e| e.to_string())?;
    let writer = pty_pair.master.take_writer().map_err(|e| e.to_string())?;

    // Cancelled flag: set by close_pty so the reader thread won't emit pty-exit
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);
    let detached = Arc::new(AtomicBool::new(false));
    let detached_clone = Arc::clone(&detached);
    let output_buffer = Arc::new(Mutex::new(String::new()));
    let output_buffer_clone = Arc::clone(&output_buffer);
    let replay_offset = Arc::new(Mutex::new(0));
    let replay_offset_clone = Arc::clone(&replay_offset);

    // Store in global state — _master keeps the PTY master fd alive for the session duration
    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(
        session_id.clone(),
        PtySession {
            writer,
            child,
            _master: pty_pair.master,
            cancelled,
            db_session_id,
            connection_name: connection.name.clone(),
            detached,
            output_buffer,
            replay_offset,
            popout_window: None,
        },
    );
    drop(sessions); // release lock before spawning thread

    // Start background thread to read from PTY master and emit to frontend
    let session_id_clone = session_id.clone();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let str_data = String::from_utf8_lossy(&buf[..n]).to_string();
                    append_to_output_buffer(&output_buffer_clone, &replay_offset_clone, &str_data);

                    if !detached_clone.load(Ordering::SeqCst) {
                        #[derive(Clone, Serialize)]
                        struct PtyPayload {
                            session_id: String,
                            data: String,
                        }
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyPayload {
                                session_id: session_id_clone.clone(),
                                data: str_data,
                            },
                        );
                    }
                }
                Err(_) => break,
            }
        }

        // Only emit pty-exit if this was NOT a manual close (avoids ghost events)
        if !cancelled_clone.load(Ordering::SeqCst) {
            let _ = app_handle.emit("pty-exit", session_id_clone.clone());
        }
    });

    Ok(())
}

#[tauri::command]
pub fn write_pty(
    state: State<'_, PtyState>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(session) = sessions.get_mut(&session_id) {
        session
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        session.writer.flush().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("PTY session '{}' not found", session_id))
    }
}

#[tauri::command]
pub fn resize_pty(
    _state: State<'_, PtyState>,
    _session_id: String,
    _cols: u16,
    _rows: u16,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn seal_session_ui(state: State<'_, PtyState>, session_id: String) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;
    seal_replay_offset(session);
    Ok(())
}

#[tauri::command]
pub fn detach_pty(state: State<'_, PtyState>, session_id: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is already detached", session_id));
    }

    session.detached.store(true, Ordering::SeqCst);
    session.popout_window = None;
    Ok(())
}

#[tauri::command]
pub fn open_terminal_window(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    let label = format!("terminal-{session_id}");

    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
        return Ok(());
    }

    {
        let mut sessions = state.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;
        session.detached.store(false, Ordering::SeqCst);
        session.popout_window = Some(label.clone());
    }

    let window_title = format!("{title} — Bayesian SSH");
    WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App(format!("/terminal/{session_id}").into()),
    )
    .title(window_title)
    .inner_size(960.0, 640.0)
    .min_inner_size(480.0, 320.0)
    .decorations(false)
    .background_color(tauri::window::Color::from((12, 13, 18, 255)))
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn claim_popout_session(
    state: State<'_, PtyState>,
    session_id: String,
    window_label: String,
) -> Result<ReattachSessionInfo, String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

    match session.popout_window.as_deref() {
        Some(existing) if existing != window_label.as_str() => {
            return Err(format!(
                "PTY session '{session_id}' is attached to another window"
            ));
        }
        None => session.popout_window = Some(window_label),
        _ => {}
    }

    session.detached.store(false, Ordering::SeqCst);
    let buffered_output = take_full_replay(session);

    Ok(ReattachSessionInfo {
        session_id: session_id.clone(),
        connection_name: session.connection_name.clone(),
        buffered_output,
    })
}

#[tauri::command]
pub fn list_detached_sessions(
    state: State<'_, PtyState>,
) -> Result<Vec<DetachedSessionInfo>, String> {
    let sessions = state.sessions.lock().unwrap();
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
    let sessions = state.sessions.lock().unwrap();
    Ok(sessions
        .iter()
        .filter_map(|(session_id, session)| {
            session
                .popout_window
                .as_ref()
                .map(|window_label| PopoutSessionInfo {
                    session_id: session_id.clone(),
                    connection_name: session.connection_name.clone(),
                    window_label: window_label.clone(),
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
) -> Result<ReattachSessionInfo, String> {
    let info = {
        let mut sessions = state.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

        if session.popout_window.as_deref() != Some(window_label.as_str()) {
            return Err(format!("PTY session '{session_id}' is not in this window"));
        }

        session.popout_window = None;
        session.detached.store(false, Ordering::SeqCst);
        let buffered_output = take_full_replay(session);

        ReattachSessionInfo {
            session_id: session_id.clone(),
            connection_name: session.connection_name.clone(),
            buffered_output,
        }
    };

    let _ = app.emit("session-docked", info.clone());

    if let Some(window) = app.get_webview_window(&window_label) {
        let _ = window.destroy();
    }

    Ok(info)
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
        let sessions = state.sessions.lock().unwrap();
        sessions
            .get(&session_id)
            .and_then(|session| session.popout_window.clone())
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
) -> Result<ReattachSessionInfo, String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("PTY session '{}' not found", session_id))?;

    if !session.detached.load(Ordering::SeqCst) {
        return Err(format!("PTY session '{}' is not detached", session_id));
    }

    session.detached.store(false, Ordering::SeqCst);
    session.popout_window = None;
    let buffered_output = take_full_replay(session);

    Ok(ReattachSessionInfo {
        session_id: session_id.clone(),
        connection_name: session.connection_name.clone(),
        buffered_output,
    })
}

#[tauri::command]
pub fn count_active_sessions(state: State<'_, PtyState>) -> Result<usize, String> {
    Ok(state.sessions.lock().unwrap().len())
}

#[tauri::command]
pub fn close_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    close_window: Option<bool>,
) -> Result<(), String> {
    let session = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions.remove(&session_id)
    };

    if let Some(mut session) = session {
        session.cancelled.store(true, Ordering::SeqCst);

        if close_window.unwrap_or(true) {
            if let Some(label) = session.popout_window.clone() {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.close();
                }
            }
        }

        let _ = session.child.kill();

        let db_session_id = session.db_session_id;
        let mut child = session.child;
        std::thread::spawn(move || {
            let exit_code = child
                .wait()
                .map(|status| status.exit_code() as i32)
                .unwrap_or(-1);
            finalize_db_session(db_session_id, exit_code);
        });

        let _ = app.emit("session-closed", session_id);
    }

    Ok(())
}

#[tauri::command]
pub fn close_all_ptys(app: AppHandle, state: State<'_, PtyState>) -> Result<usize, String> {
    let session_ids: Vec<String> = {
        let sessions = state.sessions.lock().unwrap();
        sessions.keys().cloned().collect()
    };

    let count = session_ids.len();
    for session_id in session_ids {
        let _ = close_pty(app.clone(), state.clone(), session_id, None);
    }

    Ok(count)
}
