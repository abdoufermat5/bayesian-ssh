use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;
use chrono::Utc;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use serde::{Serialize, Deserialize};

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;
use bayesian_ssh::models::{Connection, ConnectionStats, SessionHistoryEntry};

// State for active PTY sessions
pub struct PtyState {
    pub sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

pub struct PtySession {
    pub writer: Box<dyn std::io::Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
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
        created_at: Utc::now(), // will keep or let update
        last_used: None,
        tags: Vec::new(),
        aliases: Vec::new(),
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

    db.get_session_history(None, limit.unwrap_or(50), None, false).map_err(|e| e.to_string())
}

// Native Dialogs

#[tauri::command]
pub fn pick_key_file() -> Result<Option<String>, String> {
    let file = rfd::FileDialog::new()
        .set_title("Select SSH Private Key")
        .pick_file();

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

    let reader = pty_pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty_pair.master.take_writer().map_err(|e| e.to_string())?;

    // Store in global state
    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(session_id.clone(), PtySession {
        writer,
        child,
    });

    // Start background thread to read from PTY master and emit to frontend
    let session_id_clone = session_id.clone();
    let app_handle = app.clone();
    
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let data = &buf[..n];
            // Send base64 or custom string
            let str_data = String::from_utf8_lossy(data).to_string();
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
        
        // When finished, clean up session
        let _ = app_handle.emit("pty-exit", session_id_clone.clone());
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
pub fn close_pty(
    state: State<'_, PtyState>,
    session_id: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(mut session) = sessions.remove(&session_id) {
        let _ = session.child.kill();
        Ok(())
    } else {
        Err(format!("PTY session '{}' not found", session_id))
    }
}
