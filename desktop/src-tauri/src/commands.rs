use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use serde::{Serialize, Deserialize};

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::{Connection, ConnectionStats, Session, SessionHistoryEntry};

// State for active PTY sessions
pub struct PtyState {
    pub sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

const MAX_DETACHED_BUFFER_BYTES: usize = 512 * 1024;

pub struct PtySession {
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Keeps the PTY master alive for the session's lifetime.
    pub _master: Box<dyn portable_pty::MasterPty + Send>,
    /// Set to true by close_pty so the reader thread suppresses the pty-exit event.
    pub cancelled: Arc<AtomicBool>,
    /// Database session id when session logging is enabled.
    pub db_session_id: Option<Uuid>,
    pub connection_name: String,
    pub detached: Arc<AtomicBool>,
    pub output_buffer: Arc<Mutex<String>>,
    pub replay_offset: Arc<Mutex<usize>>,
    pub popout_window: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DetachedSessionInfo {
    pub session_id: String,
    pub connection_name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct PopoutSessionInfo {
    pub session_id: String,
    pub connection_name: String,
    pub window_label: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ReattachSessionInfo {
    pub session_id: String,
    pub connection_name: String,
    pub buffered_output: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct PopoutMainOverlap {
    pub overlaps: bool,
    pub overlap_ratio: f64,
    pub center_over_main: bool,
    pub should_dock: bool,
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
    if let (Ok(buffer), Ok(mut offset)) = (
        session.output_buffer.lock(),
        session.replay_offset.lock(),
    ) {
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnvInfo {
    pub name: String,
    pub is_active: bool,
}

// Environment Management Commands

#[tauri::command]
pub fn get_active_env() -> Result<String, String> {
    Ok(AppConfig::get_active_env())
}

#[tauri::command]
pub fn set_active_env(name: String) -> Result<(), String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");
    
    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() && name != "default" {
        return Err(format!("Environment '{}' does not exist.", name));
    }

    AppConfig::set_active_env(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_environments() -> Result<Vec<EnvInfo>, String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");
    
    let active_env = AppConfig::get_active_env();
    let mut envs = vec![EnvInfo {
        name: "default".to_string(),
        is_active: active_env == "default",
    }];

    if envs_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&envs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let name_str = name.to_string();
                        if name_str != "default" {
                            envs.push(EnvInfo {
                                name: name_str.clone(),
                                is_active: name_str == active_env,
                            });
                        }
                    }
                }
            }
        }
    }

    envs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(envs)
}

#[tauri::command]
pub fn create_environment(name: String) -> Result<(), String> {
    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");
    
    let env_dir = envs_dir.join(&name);
    if env_dir.exists() {
        return Err(format!("Environment '{}' already exists.", name));
    }

    std::fs::create_dir_all(&env_dir).map_err(|e| e.to_string())?;

    let config = AppConfig::default_for_env(&name);
    config.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn remove_environment(name: String) -> Result<(), String> {
    let active_env = AppConfig::get_active_env();
    if name == active_env {
        return Err("Cannot remove the currently active environment.".to_string());
    }
    if name == "default" {
        return Err("Cannot remove the built-in 'default' environment.".to_string());
    }

    let envs_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments");
    
    let env_dir = envs_dir.join(&name);
    if !env_dir.exists() {
        return Err(format!("Environment '{}' does not exist.", name));
    }

    std::fs::remove_dir_all(&env_dir).map_err(|e| e.to_string())?;
    Ok(())
}

// Connection Management Commands

#[tauri::command]
pub fn get_connections(query: Option<String>, tag_filter: Option<String>) -> Result<Vec<Connection>, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    if let Some(q) = query {
        if !q.trim().is_empty() {
            return db.search_connections(&q, 100, &config.search_mode).map_err(|e| e.to_string());
        }
    }

    db.list_connections(tag_filter.as_deref(), false).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_connection(
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    bastion_user: Option<String>,
    key_path: Option<String>,
    tags: Vec<String>
) -> Result<(), String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    let default_u = config.default_user.clone();
    let default_p = config.default_port;
    let default_k = config.use_kerberos_by_default;

    let mut connection = Connection::new(
        name,
        host,
        user.unwrap_or(default_u),
        port.unwrap_or(default_p),
        bastion,
        bastion_user,
        kerberos.unwrap_or(default_k),
        key_path,
    );

    for tag in tags {
        connection.add_tag(tag);
    }

    db.add_connection(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn edit_connection(
    id: String,
    name: String,
    host: String,
    user: String,
    port: u16,
    kerberos: bool,
    bastion: Option<String>,
    bastion_user: Option<String>,
    key_path: Option<String>,
    tags: Vec<String>
) -> Result<(), String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    let uuid = Uuid::parse_str(&id).map_err(|e: uuid::Error| e.to_string())?;

    let existing = db
        .get_connection(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{}' not found", id))?;

    if existing.id != uuid {
        return Err("Connection id mismatch.".to_string());
    }

    let mut connection = Connection {
        id: uuid,
        name,
        host,
        user,
        port,
        bastion,
        bastion_user,
        use_kerberos: kerberos,
        key_path,
        created_at: existing.created_at,
        last_used: existing.last_used,
        tags: Vec::new(),
        aliases: existing.aliases,
    };

    for tag in tags {
        connection.add_tag(tag);
    }

    db.update_connection(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_connection(id_or_name: String) -> Result<(), String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    db.remove_connection(&id_or_name).map_err(|e| e.to_string())?;
    Ok(())
}

// History & Stats Commands

#[tauri::command]
pub fn get_stats() -> Result<ConnectionStats, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    db.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_history(limit: Option<usize>) -> Result<Vec<SessionHistoryEntry>, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    let effective_limit = limit.unwrap_or(config.max_history_size.max(1));
    db.get_session_history(None, effective_limit, None, false).map_err(|e| e.to_string())
}

// Native Dialogs

#[tauri::command]
pub fn pick_key_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let ssh_dir = dirs::home_dir().map(|h| h.join(".ssh"));
    
    let mut dialog = rfd::FileDialog::new()
        .set_title("Select SSH Private Key")
        .set_parent(&window);
        
    if let Some(ref path) = ssh_dir {
        if path.exists() {
            dialog = dialog.set_directory(path);
        }
    }
    
    // Temporarily minimize window to force dialog to the foreground
    let _ = window.minimize();
    let file = dialog.pick_file();
    let _ = window.unminimize();
    let _ = window.set_focus();
    
    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

// Interactive PTY / Terminal Spawning

#[tauri::command]
pub fn spawn_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    session_id: String,
    connection_name: String,
) -> Result<(), String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;

    let connection = db.get_connection(&connection_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{}' not found", connection_name))?;

    // Record last used
    let mut updated_conn = connection.clone();
    updated_conn.update_last_used();
    let _ = db.update_connection(&updated_conn);

    // Build the SSH command arguments
    let pty_system = native_pty_system();
    
    // We will build shell execution: e.g. "ssh" with args
    let mut cmd_builder = CommandBuilder::new("ssh");
    
    // Inherit env vars so Kerberos tickets (KRB5CCNAME) and ssh-agent (SSH_AUTH_SOCK) are passed down
    for (key, val) in std::env::vars() {
        cmd_builder.env(key, val);
    }
    
    if connection.use_kerberos {
        cmd_builder.arg("-t");
        cmd_builder.arg("-A");
        cmd_builder.arg("-K");
    }
    
    if let Some(key) = &connection.key_path {
        cmd_builder.arg("-i");
        cmd_builder.arg(key);
    }
    
    if let Some(bastion) = &connection.bastion {
        let bastion_user = connection.bastion_user.as_deref().unwrap_or(&connection.user);
        cmd_builder.arg("-p");
        cmd_builder.arg("22");
        cmd_builder.arg(format!("{}@{}", bastion_user, bastion));
        cmd_builder.arg(format!("{}@{}", connection.user, connection.host));
    } else {
        cmd_builder.arg("-p");
        cmd_builder.arg(connection.port.to_string());
        cmd_builder.arg(format!("{}@{}", connection.user, connection.host));
    }

    // Open PTY Pair
    let pty_pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).map_err(|e| e.to_string())?;

    let child = pty_pair.slave.spawn_command(cmd_builder).map_err(|e| e.to_string())?;

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

    let reader = pty_pair.master.try_clone_reader().map_err(|e| e.to_string())?;
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
    sessions.insert(session_id.clone(), PtySession {
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
    });
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
                    append_to_output_buffer(
                        &output_buffer_clone,
                        &replay_offset_clone,
                        &str_data,
                    );

                    if !detached_clone.load(Ordering::SeqCst) {
                        #[derive(Clone, Serialize)]
                        struct PtyPayload {
                            session_id: String,
                            data: String,
                        }
                        let _ = app_handle.emit("pty-output", PtyPayload {
                            session_id: session_id_clone.clone(),
                            data: str_data,
                        });
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
        session.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
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
    // Note: portable-pty does not easily let us resize a spawned session's Master side without keeping the PtyPair,
    // but in Unix we can adjust terminal size using standard ioctls if supported.
    // For standard window resize, we can omit or run a resize command if supported by the pty_system.
    // We'll keep this as a stub that returns Ok since Xterm.js handles visual resizing natively.
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
pub fn list_detached_sessions(state: State<'_, PtyState>) -> Result<Vec<DetachedSessionInfo>, String> {
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
            session.popout_window.as_ref().map(|window_label| PopoutSessionInfo {
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
    window
        .set_focus()
        .map_err(|e| e.to_string())
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

// SSH Agent Integration Commands
use std::process::Command;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AgentStatus {
    pub active: bool,
    pub socket_path: Option<String>,
    pub keys: Vec<String>,
}

#[tauri::command]
pub fn get_agent_status() -> Result<AgentStatus, String> {
    let socket = std::env::var("SSH_AUTH_SOCK").ok();
    let active = socket.is_some();
    
    let mut keys = Vec::new();
    if active {
        // Run ssh-add -l to list loaded keys
        let output = Command::new("ssh-add")
            .arg("-l")
            .output();
            
        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if !line.trim().is_empty() {
                        keys.push(line.to_string());
                    }
                }
            }
        }
    }
    
    Ok(AgentStatus {
        active,
        socket_path: socket,
        keys,
    })
}

#[tauri::command]
pub fn start_agent() -> Result<AgentStatus, String> {
    if std::env::var("SSH_AUTH_SOCK").is_ok() {
        return get_agent_status();
    }
    
    let output = Command::new("ssh-agent")
        .arg("-s")
        .output()
        .map_err(|e| format!("Failed to start ssh-agent: {}", e))?;
        
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut socket_path = None;
    
    for line in stdout.lines() {
        // e.g. SSH_AUTH_SOCK=/tmp/ssh-XXXXXX/agent.XXXX; export SSH_AUTH_SOCK;
        if line.starts_with("SSH_AUTH_SOCK=") {
            if let Some(end) = line.find(';') {
                let val = &line["SSH_AUTH_SOCK=".len()..end];
                socket_path = Some(val.to_string());
                std::env::set_var("SSH_AUTH_SOCK", val);
            }
        }
        // e.g. SSH_AGENT_PID=XXXXX; export SSH_AGENT_PID;
        if line.starts_with("SSH_AGENT_PID=") {
            if let Some(end) = line.find(';') {
                let val = &line["SSH_AGENT_PID=".len()..end];
                std::env::set_var("SSH_AGENT_PID", val);
            }
        }
    }
    
    if socket_path.is_none() {
        return Err("Failed to parse ssh-agent environment variables".to_string());
    }
    
    get_agent_status()
}

#[tauri::command]
pub fn add_key_to_agent(key_path: String) -> Result<String, String> {
    let output = Command::new("ssh-add")
        .arg(&key_path)
        .output()
        .map_err(|e| format!("Failed to execute ssh-add: {}", e))?;
        
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
        if err_msg.is_empty() {
            Err(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(err_msg)
        }
    }
}

// Desktop App Settings
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct DesktopSettings {
    pub theme: String, // "zinc" | "cyberpunk" | "oled" | "slate"
    pub auto_start_agent: bool,
    pub custom_agent_socket: Option<String>,
    #[serde(default = "default_kerberos_warn_minutes")]
    pub kerberos_warn_minutes: u32,
    #[serde(default = "default_monitor_kerberos")]
    pub monitor_kerberos: bool,
    pub default_user: String,
    pub default_port: u16,
    pub fuzzy_search: bool,
    pub default_key_path: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_onboarding_complete")]
    pub onboarding_complete: bool,
}

fn default_timezone() -> String {
    "system".to_string()
}

fn default_kerberos_warn_minutes() -> u32 {
    15
}

fn default_monitor_kerberos() -> bool {
    true
}

fn default_onboarding_complete() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkspaceInfo {
    pub active_env: String,
    pub config_root: String,
    pub env_dir: String,
    pub config_path: String,
    pub database_path: String,
    pub ssh_config_path: Option<String>,
    pub default_user: String,
    pub default_port: u16,
    pub search_mode: String,
    pub log_level: String,
    pub auto_save_history: bool,
    pub max_history_size: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkspaceConfigUpdate {
    pub default_user: Option<String>,
    pub default_port: Option<u16>,
    pub ssh_config_path: Option<String>,
    pub search_mode: Option<String>,
    pub log_level: Option<String>,
    pub auto_save_history: Option<bool>,
    pub max_history_size: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OnboardingPayload {
    pub profile_name: String,
    pub create_profile: bool,
    pub default_user: String,
    pub default_port: u16,
    pub ssh_config_path: Option<String>,
    pub theme: String,
    pub auto_start_agent: bool,
    pub import_ssh_config: bool,
    pub fuzzy_search: bool,
}

fn bayesian_config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
}

fn sync_search_mode_to_config(fuzzy: bool) -> String {
    if fuzzy {
        "fuzzy".to_string()
    } else {
        "bayesian".to_string()
    }
}

#[tauri::command]
pub fn get_workspace_info() -> Result<WorkspaceInfo, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let config_root = bayesian_config_root();
    let env_dir = config_root.join("environments").join(&config.environment);

    Ok(WorkspaceInfo {
        active_env: config.environment.clone(),
        config_root: config_root.to_string_lossy().to_string(),
        env_dir: env_dir.to_string_lossy().to_string(),
        config_path: env_dir.join("config.json").to_string_lossy().to_string(),
        database_path: config.database_path.to_string_lossy().to_string(),
        ssh_config_path: config
            .ssh_config_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        default_user: config.default_user.clone(),
        default_port: config.default_port,
        search_mode: config.search_mode.clone(),
        log_level: config.log_level.clone(),
        auto_save_history: config.auto_save_history,
        max_history_size: config.max_history_size,
    })
}

#[tauri::command]
pub fn save_workspace_config(update: WorkspaceConfigUpdate) -> Result<(), String> {
    let mut config = AppConfig::load(None).map_err(|e| e.to_string())?;

    if let Some(user) = update.default_user {
        config.default_user = user;
    }
    if let Some(port) = update.default_port {
        config.default_port = port;
    }
    if let Some(path) = update.ssh_config_path {
        config.ssh_config_path = if path.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        };
    }
    if let Some(mode) = update.search_mode {
        if mode == "bayesian" || mode == "fuzzy" {
            config.search_mode = mode;
        }
    }
    if let Some(log_level) = update.log_level {
        config.log_level = log_level;
    }
    if let Some(auto_save) = update.auto_save_history {
        config.auto_save_history = auto_save;
    }
    if let Some(max_size) = update.max_history_size {
        config.max_history_size = max_size.clamp(50, 100_000);
    }

    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn needs_onboarding() -> Result<bool, String> {
    let settings_file = bayesian_config_root().join("desktop_settings.json");

    if !settings_file.exists() {
        return Ok(true);
    }

    let content = std::fs::read_to_string(&settings_file).map_err(|e| e.to_string())?;
    let settings: DesktopSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(!settings.onboarding_complete)
}

#[tauri::command]
pub fn complete_onboarding(payload: OnboardingPayload) -> Result<usize, String> {
    let profile = payload.profile_name.trim().to_string();
    if profile.is_empty() {
        return Err("Profile name is required.".to_string());
    }

    if payload.create_profile && profile != "default" {
        create_environment(profile.clone())?;
    }

    set_active_env(profile)?;

    let default_user = payload.default_user.clone();
    let default_port = payload.default_port;

    save_workspace_config(WorkspaceConfigUpdate {
        default_user: Some(default_user.clone()),
        default_port: Some(default_port),
        ssh_config_path: payload.ssh_config_path.clone(),
        search_mode: Some(sync_search_mode_to_config(payload.fuzzy_search)),
        log_level: None,
        auto_save_history: None,
        max_history_size: None,
    })?;

    let mut settings = if bayesian_config_root().join("desktop_settings.json").exists() {
        load_desktop_settings()?
    } else {
        DesktopSettings::default()
    };

    settings.theme = payload.theme;
    settings.auto_start_agent = payload.auto_start_agent;
    settings.default_user = default_user;
    settings.default_port = default_port;
    settings.fuzzy_search = payload.fuzzy_search;
    settings.onboarding_complete = true;
    save_desktop_settings(settings)?;

    if payload.auto_start_agent {
        let _ = start_agent();
    }

    if payload.import_ssh_config {
        import_ssh_config(payload.ssh_config_path)
    } else {
        Ok(0)
    }
}

#[tauri::command]
pub fn import_ssh_config(file: Option<String>) -> Result<usize, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let ssh_config_path = if let Some(file) = file.filter(|f| !f.trim().is_empty()) {
        PathBuf::from(file)
    } else {
        config
            .ssh_config_path
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".ssh/config")))
            .ok_or_else(|| "No SSH config path configured.".to_string())?
    };

    if !ssh_config_path.exists() {
        return Err(format!(
            "SSH config file not found: {}",
            ssh_config_path.display()
        ));
    }

    let db = Database::new(&config).map_err(|e| e.to_string())?;
    let content = std::fs::read_to_string(&ssh_config_path).map_err(|e| e.to_string())?;

    let mut imported_count = 0usize;
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port: Option<u16> = None;
    let mut current_identity_file: Option<String> = None;

    let flush_host = |host: String,
                      hostname: Option<String>,
                      user: Option<String>,
                      port: Option<u16>,
                      identity: Option<String>,
                      db: &Database,
                      cfg: &AppConfig|
     -> Result<bool, String> {
        if host.contains('*') || host.contains('?') {
            return Ok(false);
        }

        if db
            .get_connection(&host)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Ok(false);
        }

        let actual_host = hostname.unwrap_or_else(|| host.clone());
        let mut connection = Connection::new(
            host,
            actual_host,
            user.unwrap_or_else(|| cfg.default_user.clone()),
            port.unwrap_or(cfg.default_port),
            None,
            None,
            false,
            identity,
        );
        connection.add_tag("imported".to_string());
        db.add_connection(&connection).map_err(|e| e.to_string())?;
        Ok(true)
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(host) = line.strip_prefix("Host ") {
            if let Some(prev) = current_host.take() {
                if flush_host(
                    prev,
                    current_hostname.take(),
                    current_user.take(),
                    current_port.take(),
                    current_identity_file.take(),
                    &db,
                    &config,
                )? {
                    imported_count += 1;
                }
            }
            current_host = Some(host.trim().to_string());
            current_hostname = None;
            current_user = None;
            current_port = None;
            current_identity_file = None;
        } else if let Some(user) = line.strip_prefix("User ") {
            current_user = Some(user.trim().to_string());
        } else if let Some(port) = line.strip_prefix("Port ") {
            if let Ok(port) = port.trim().parse::<u16>() {
                current_port = Some(port);
            }
        } else if let Some(hostname) = line.strip_prefix("HostName ") {
            current_hostname = Some(hostname.trim().to_string());
        } else if let Some(identity_file) = line.strip_prefix("IdentityFile ") {
            current_identity_file = Some(identity_file.trim().to_string());
        }
    }

    if let Some(host) = current_host {
        if flush_host(
            host,
            current_hostname,
            current_user,
            current_port,
            current_identity_file,
            &db,
            &config,
        )? {
            imported_count += 1;
        }
    }

    Ok(imported_count)
}

#[tauri::command]
pub fn pick_ssh_config_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let ssh_dir = dirs::home_dir().map(|h| h.join(".ssh"));

    let mut dialog = rfd::FileDialog::new()
        .set_title("Select SSH Config File")
        .add_filter("SSH Config", &["config", "conf"])
        .set_parent(&window);

    if let Some(ref path) = ssh_dir {
        if path.exists() {
            dialog = dialog.set_directory(path);
        }
    }

    let _ = window.minimize();
    let file = dialog.pick_file();
    let _ = window.unminimize();
    let _ = window.set_focus();

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

impl Default for DesktopSettings {
    fn default() -> Self {
        // Try to detect current SSH_AUTH_SOCK from environment
        let current_sock = std::env::var("SSH_AUTH_SOCK").ok().filter(|s| !s.is_empty());
        Self {
            theme: "zinc".to_string(),
            auto_start_agent: false,
            custom_agent_socket: current_sock,
            kerberos_warn_minutes: default_kerberos_warn_minutes(),
            monitor_kerberos: default_monitor_kerberos(),
            default_user: std::env::var("USER").unwrap_or_else(|_| "root".to_string()),
            default_port: 22,
            fuzzy_search: false,
            default_key_path: None,
            timezone: default_timezone(),
            onboarding_complete: false,
        }
    }
}

#[tauri::command]
pub fn load_desktop_settings() -> Result<DesktopSettings, String> {
    let settings_file = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("desktop_settings.json");
        
    if settings_file.exists() {
        let content = std::fs::read_to_string(&settings_file).map_err(|e| e.to_string())?;
        let mut settings: DesktopSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        
        // Apply custom agent socket if specified on load
        if let Some(ref sock) = settings.custom_agent_socket {
            if !sock.trim().is_empty() {
                std::env::set_var("SSH_AUTH_SOCK", sock);
            }
        } else {
            // Pre-fill from running environment so settings panel shows the live socket
            settings.custom_agent_socket = std::env::var("SSH_AUTH_SOCK").ok().filter(|s| !s.is_empty());
        }
        
        Ok(settings)
    } else {
        Ok(DesktopSettings {
            onboarding_complete: false,
            ..DesktopSettings::default()
        })
    }
}

#[tauri::command]
pub fn save_desktop_settings(settings: DesktopSettings) -> Result<(), String> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh");
        
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let settings_file = config_dir.join("desktop_settings.json");
    
    let content = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_file, content).map_err(|e| e.to_string())?;

    // Keep workspace search mode aligned with desktop fuzzy preference
    if let Ok(mut config) = AppConfig::load(None) {
        config.search_mode = sync_search_mode_to_config(settings.fuzzy_search);
        let _ = config.save();
    }

    // Apply custom agent socket environment variable immediately
    if let Some(ref sock) = settings.custom_agent_socket {
        if !sock.trim().is_empty() {
            std::env::set_var("SSH_AUTH_SOCK", sock);
        } else {
            // Restore original parent agent if any, or remove
            std::env::remove_var("SSH_AUTH_SOCK");
        }
    } else {
        std::env::remove_var("SSH_AUTH_SOCK");
    }
    
    Ok(())
}


