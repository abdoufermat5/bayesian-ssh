use portable_pty::Child;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::database::Database;

pub mod agent;
pub mod ai;
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
pub mod sources;
pub mod teams;
pub mod upload;

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
pub use sources::*;
pub use teams::*;
pub use upload::*;

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
/// The replay buffer may grow this far past [`MAX_DETACHED_BUFFER_BYTES`]
/// before it is trimmed, so the front-drain happens once per 64 KiB of output
/// instead of on every chunk.
pub const REPLAY_TRIM_SLACK_BYTES: usize = 64 * 1024;

/// Label of the main application window.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Label of the pop-out window that owns a session, shared with the session's
/// reader thread so live output is routed to that window only. `None` means
/// the main window owns the session.
#[derive(Clone, Default)]
pub struct PopoutOwner(Arc<Mutex<Option<String>>>);

impl PopoutOwner {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get(&self) -> Option<String> {
        self.lock().clone()
    }

    pub fn set(&self, label: Option<String>) {
        *self.lock() = label;
    }

    pub fn is_none(&self) -> bool {
        self.lock().is_none()
    }

    /// Run `f` with the current owner without cloning the label.
    pub fn with<R>(&self, f: impl FnOnce(Option<&str>) -> R) -> R {
        f(self.lock().as_deref())
    }
}

pub struct PtySession {
    /// Input queue drained by the session's writer thread, so `write_pty`
    /// never blocks the IPC thread on a full PTY and keeps keystroke order.
    pub input: Sender<String>,
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
    pub popout_window: PopoutOwner,
    pub recorder: Option<bayesian_ssh::services::recording::SharedRecorder>,
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
    #[serde(default = "default_theme", deserialize_with = "deserialize_theme")]
    pub theme: String, // "dark" | "light" | "system"
    #[serde(default = "default_density")]
    pub density: String, // "compact" | "comfortable" | "spacious"
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
    /// Ctrl+C copies when text is selected (otherwise it always sends SIGINT).
    #[serde(default)]
    pub terminal_smart_copy: bool,
    #[serde(default = "default_terminal_scrollback")]
    pub terminal_scrollback: u32,
    #[serde(default = "default_sftp_show_hidden_files")]
    pub sftp_show_hidden_files: bool,
    #[serde(default = "default_sftp_default_remote_path")]
    pub sftp_default_remote_path: String,
    #[serde(default = "default_enable_sftp")]
    pub enable_sftp: bool,
    #[serde(default)]
    pub record_sessions: bool,
    #[serde(default)]
    pub record_tags: Vec<String>,
}

pub fn default_sftp_show_hidden_files() -> bool {
    true
}

pub fn default_sftp_default_remote_path() -> String {
    "/".to_string()
}

pub fn default_enable_sftp() -> bool {
    true
}

pub fn default_timezone() -> String {
    "system".to_string()
}
pub fn default_theme() -> String {
    "dark".to_string()
}

/// Themes from earlier versions load as the theme that replaced them.
pub fn deserialize_theme<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(match s.as_str() {
        "zinc" | "cyberpunk" | "oled" | "slate" => "dark".to_string(),
        "paper" => "light".to_string(),
        _ => s,
    })
}

pub fn default_density() -> String {
    "comfortable".to_string()
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
            theme: default_theme(),
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
            terminal_smart_copy: false,
            terminal_scrollback: default_terminal_scrollback(),
            sftp_show_hidden_files: default_sftp_show_hidden_files(),
            sftp_default_remote_path: default_sftp_default_remote_path(),
            enable_sftp: default_enable_sftp(),
            record_sessions: false,
            record_tags: Vec::new(),
            density: default_density(),
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

#[cfg(test)]
mod tests {
    use super::DesktopSettings;

    /// Settings saved by a version that still had snippets must keep loading.
    #[test]
    fn settings_with_the_removed_snippet_option_still_load() {
        let mut saved = serde_json::to_value(DesktopSettings::default()).unwrap();
        saved["confirm_snippet_execution"] = serde_json::json!(false);
        let loaded: DesktopSettings = serde_json::from_value(saved).unwrap();
        assert!(serde_json::to_value(&loaded)
            .unwrap()
            .get("confirm_snippet_execution")
            .is_none());
    }

    #[test]
    fn settings_migrate_removed_themes() {
        for (stored, expected) in [
            ("zinc", "dark"),
            ("cyberpunk", "dark"),
            ("oled", "dark"),
            ("slate", "dark"),
            ("paper", "light"),
            ("system", "system"),
            ("light", "light"),
        ] {
            let mut saved = serde_json::to_value(DesktopSettings::default()).unwrap();
            saved["theme"] = serde_json::json!(stored);
            let loaded: DesktopSettings = serde_json::from_value(saved).unwrap();
            assert_eq!(loaded.theme, expected, "stored {stored}");
        }
    }

    #[test]
    fn settings_defaults_density_to_comfortable() {
        let saved = serde_json::json!({
            "auto_start_agent": false,
            "default_user": "root",
            "default_port": 22,
            "fuzzy_search": false
        });
        let loaded: DesktopSettings = serde_json::from_value(saved).unwrap();
        assert_eq!(loaded.density, "comfortable");
    }

    #[test]
    fn settings_saved_with_removed_fields_still_load() {
        // `enable_tunneling` went away with the Tunnels view.
        let mut saved = serde_json::to_value(DesktopSettings::default()).unwrap();
        saved["enable_tunneling"] = serde_json::json!(false);
        let loaded: Result<DesktopSettings, _> = serde_json::from_value(saved);
        assert!(loaded.is_ok());
    }
}
