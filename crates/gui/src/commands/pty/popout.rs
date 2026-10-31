//! Pop-out terminal windows and their geometry.

use super::output::PtyChunk;
use super::replay::{switch_sink_off, switch_sink_on};
use crate::commands::CommandError;
use crate::commands::{
    DockedSessionInfo, PopoutMainOverlap, PopoutSessionInfo, PtyState, ReattachSessionInfo,
};
use std::sync::atomic::Ordering;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub(super) struct WindowRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub(super) fn window_outer_rect(window: &tauri::WebviewWindow) -> Result<WindowRect, CommandError> {
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    Ok(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

pub(super) fn rect_intersection_area(a: &WindowRect, b: &WindowRect) -> i64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    if right <= left || bottom <= top {
        return 0;
    }
    (right - left) as i64 * (bottom - top) as i64
}

pub(super) fn rect_center_over(a: &WindowRect, container: &WindowRect) -> bool {
    let center_x = a.x + a.width as i32 / 2;
    let center_y = a.y + a.height as i32 / 2;
    center_x >= container.x
        && center_x <= container.x + container.width as i32
        && center_y >= container.y
        && center_y <= container.y + container.height as i32
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
) -> Result<(), CommandError> {
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
        return Err(e.to_string().into());
    }

    Ok(())
}

#[tauri::command]
pub fn claim_popout_session(
    state: State<'_, PtyState>,
    session_id: String,
    window_label: String,
    output: Channel<PtyChunk>,
) -> Result<ReattachSessionInfo, CommandError> {
    let sessions = state.lock_sessions();
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

    match session.popout_window.get() {
        Some(existing) if existing != window_label => {
            return Err(format!("PTY session '{session_id}' is attached to another window").into());
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

#[tauri::command]
pub fn list_popout_sessions(
    state: State<'_, PtyState>,
) -> Result<Vec<PopoutSessionInfo>, CommandError> {
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
) -> Result<(), CommandError> {
    let info = {
        let sessions = state.lock_sessions();
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY session '{session_id}' not found"))?;

        if session.popout_window.get().as_deref() != Some(window_label.as_str()) {
            return Err(format!("PTY session '{session_id}' is not in this window").into());
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
) -> Result<PopoutMainOverlap, CommandError> {
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
) -> Result<(), CommandError> {
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
    window.set_focus().map_err(CommandError::from)
}
