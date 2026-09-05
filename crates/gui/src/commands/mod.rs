use portable_pty::Child;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;

pub mod agent;
pub mod app;
pub mod connections;
pub mod dialogs;
pub mod env;
pub mod history;
pub mod import;
pub mod pty;
pub mod security;
pub mod settings;
pub mod sftp;

pub use agent::*;
pub use app::*;
pub use connections::*;
pub use dialogs::*;
pub use env::*;
pub use history::*;
pub use import::*;
pub use pty::*;
pub use security::*;
pub use settings::*;
pub use sftp::*;

// State for active PTY sessions
pub struct PtyState {
    pub sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl PtyState {
    /// Lock the session map, surviving a poisoned mutex (a panic while the
    /// lock was held must not crash the whole GUI — the lock is only ever
    /// held briefly, so recovering the data is always safe).
    pub fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, PtySession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn get_db_and_config() -> Result<(Database, AppConfig), String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    let db = Database::new(&config).map_err(|e| e.to_string())?;
    Ok((db, config))
}

pub const MAX_DETACHED_BUFFER_BYTES: usize = 512 * 1024;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnvInfo {
    pub name: String,
    pub is_active: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct AgentStatus {
    pub active: bool,
    pub socket_path: Option<String>,
    pub keys: Vec<String>,
}

// Desktop App Settings
#[derive(Serialize, Deserialize, Clone)]
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
    #[serde(default = "default_terminal_font_family")]
    pub terminal_font_family: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u32,
    #[serde(default = "default_terminal_line_height")]
    pub terminal_line_height: f64,
    #[serde(default = "default_terminal_cursor_style")]
    pub terminal_cursor_style: String,
    #[serde(default = "default_terminal_cursor_blink")]
    pub terminal_cursor_blink: bool,
    #[serde(default = "default_terminal_copy_on_select")]
    pub terminal_copy_on_select: bool,
    #[serde(default = "default_terminal_scrollback")]
    pub terminal_scrollback: u32,
    #[serde(default = "default_sftp_show_hidden_files")]
    pub sftp_show_hidden_files: bool,
    #[serde(default = "default_sftp_default_remote_path")]
    pub sftp_default_remote_path: String,
    #[serde(default = "default_confirm_snippet_execution")]
    pub confirm_snippet_execution: bool,
    #[serde(default = "default_enable_sftp")]
    pub enable_sftp: bool,
    #[serde(default = "default_enable_tunneling")]
    pub enable_tunneling: bool,
}

pub fn default_sftp_show_hidden_files() -> bool {
    true
}

pub fn default_sftp_default_remote_path() -> String {
    "/".to_string()
}

pub fn default_confirm_snippet_execution() -> bool {
    true
}

pub fn default_enable_sftp() -> bool {
    true
}

pub fn default_enable_tunneling() -> bool {
    true
}

pub fn default_timezone() -> String {
    "system".to_string()
}

pub fn default_kerberos_warn_minutes() -> u32 {
    15
}

pub fn default_monitor_kerberos() -> bool {
    true
}

pub fn default_onboarding_complete() -> bool {
    true
}

pub fn default_terminal_font_family() -> String {
    "JetBrains Mono, Fira Code, Cascadia Code, Ubuntu Mono, DejaVu Sans Mono, Liberation Mono, Consolas, monospace".to_string()
}

pub fn default_terminal_font_size() -> u32 {
    13
}

pub fn default_terminal_line_height() -> f64 {
    1.18
}

pub fn default_terminal_cursor_style() -> String {
    "block".to_string()
}

pub fn default_terminal_cursor_blink() -> bool {
    true
}

pub fn default_terminal_copy_on_select() -> bool {
    false
}

pub fn default_terminal_scrollback() -> u32 {
    10000
}

impl Default for DesktopSettings {
    fn default() -> Self {
        // Try to detect current SSH_AUTH_SOCK from environment
        let current_sock = std::env::var("SSH_AUTH_SOCK")
            .ok()
            .filter(|s| !s.is_empty());
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
            terminal_font_family: default_terminal_font_family(),
            terminal_font_size: default_terminal_font_size(),
            terminal_line_height: default_terminal_line_height(),
            terminal_cursor_style: default_terminal_cursor_style(),
            terminal_cursor_blink: default_terminal_cursor_blink(),
            terminal_copy_on_select: default_terminal_copy_on_select(),
            terminal_scrollback: default_terminal_scrollback(),
            sftp_show_hidden_files: default_sftp_show_hidden_files(),
            sftp_default_remote_path: default_sftp_default_remote_path(),
            confirm_snippet_execution: default_confirm_snippet_execution(),
            enable_sftp: default_enable_sftp(),
            enable_tunneling: default_enable_tunneling(),
        }
    }
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

pub fn bayesian_config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
}

pub fn sync_search_mode_to_config(fuzzy: bool) -> String {
    if fuzzy {
        "fuzzy".to_string()
    } else {
        "bayesian".to_string()
    }
}
