//! TUI data models, enums, and small types

use crate::models::Connection;
use crate::services::transport::types::{ForwardHandle, RemoteEntry};
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// Active tab in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Connections,
    History,
    Config,
    Files,
    Tunnels,
}

impl Tab {
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Connections => "Connections",
            Tab::History => "History",
            Tab::Config => "Config",
            Tab::Files => "Files",
            Tab::Tunnels => "Tunnels",
        }
    }

    pub fn all() -> &'static [Tab] {
        &[
            Tab::Connections,
            Tab::History,
            Tab::Config,
            Tab::Files,
            Tab::Tunnels,
        ]
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Connections => 0,
            Tab::History => 1,
            Tab::Config => 2,
            Tab::Files => 3,
            Tab::Tunnels => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Connections,
            1 => Tab::History,
            2 => Tab::Config,
            3 => Tab::Files,
            4 => Tab::Tunnels,
            _ => Tab::Connections,
        }
    }

    pub fn next(&self) -> Self {
        Self::from_index((self.index() + 1) % Self::all().len())
    }

    pub fn prev(&self) -> Self {
        let len = Self::all().len();
        Self::from_index((self.index() + len - 1) % len)
    }
}

/// Application mode/state
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Normal browsing mode
    Normal,
    /// Search/filter mode
    Search,
    /// Help overlay
    Help,
    /// Confirmation dialog
    Confirm(ConfirmAction),
    /// Detail preview pane
    Detail,
    /// Inline edit mode
    Edit,
    /// Add new connection mode
    Add,
    /// Quick connect bar
    QuickConnect,
    /// SSH command preview
    CommandPreview,
    /// Tunnel launch dialog (entering -L spec)
    TunnelLaunch,
    /// Files tab prompt dialog (upload path / mkdir name / rename target)
    FilesPrompt(FilesPromptKind),
}

/// What the Files tab prompt dialog is collecting
#[derive(Debug, Clone, PartialEq)]
pub enum FilesPromptKind {
    /// Upload: user types a local file path
    Upload,
    /// Download: user types a local destination path
    Download { remote_path: String, is_dir: bool },
    /// Mkdir: user types a directory name (relative to current path)
    Mkdir,
    /// Rename: user types the new name; old_name is the current file name
    Rename { old_name: String },
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Delete(usize),
    BatchDelete,
    StopTunnel(usize),
    /// Delete a remote file/dir at the given path
    DeleteFile(String),
}

/// Sort field for connection list
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Name,
    Host,
    LastUsed,
    Created,
}

impl SortField {
    pub fn label(&self) -> &'static str {
        match self {
            SortField::Name => "Name",
            SortField::Host => "Host",
            SortField::LastUsed => "Last Used",
            SortField::Created => "Created",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SortField::Name => SortField::Host,
            SortField::Host => SortField::LastUsed,
            SortField::LastUsed => SortField::Created,
            SortField::Created => SortField::Name,
        }
    }
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }

    pub fn arrow(&self) -> &'static str {
        match self {
            SortDirection::Asc => "↑",
            SortDirection::Desc => "↓",
        }
    }
}

/// State for inline editing (edit or add)
#[derive(Debug, Clone)]
pub struct EditState {
    /// Working copy of the connection being edited
    pub connection: Connection,
    /// Original connection name (for DB update lookup); empty for new connections
    pub original_name: String,
    /// Whether this is a new connection (Add mode)
    pub is_new: bool,
    /// Which field is currently selected (0-8)
    pub field_index: usize,
    /// Current input buffer for the active field
    pub field_value: String,
}

impl EditState {
    pub const FIELD_COUNT: usize = 9;

    pub fn field_label(index: usize) -> &'static str {
        match index {
            0 => "Name",
            1 => "Host",
            2 => "User",
            3 => "Port",
            4 => "Bastion",
            5 => "Bastion User",
            6 => "Key Path",
            7 => "Kerberos",
            8 => "Tags",
            _ => "",
        }
    }

    pub fn field_value_str(&self, index: usize) -> String {
        match index {
            0 => self.connection.name.clone(),
            1 => self.connection.host.clone(),
            2 => self.connection.user.clone(),
            3 => self.connection.port.to_string(),
            4 => self.connection.bastion.clone().unwrap_or_default(),
            5 => self.connection.bastion_user.clone().unwrap_or_default(),
            6 => self.connection.key_path.clone().unwrap_or_default(),
            7 => {
                if self.connection.use_kerberos {
                    "yes".into()
                } else {
                    "no".into()
                }
            }
            8 => self.connection.tags.join(", "),
            _ => String::new(),
        }
    }

    /// Apply the current field_value buffer into the connection struct
    pub fn apply_field(&mut self) {
        let val = self.field_value.trim().to_string();
        match self.field_index {
            0 => self.connection.name = val,
            1 => self.connection.host = val,
            2 => self.connection.user = val,
            3 => {
                if let Ok(p) = val.parse::<u16>() {
                    self.connection.port = p;
                }
            }
            4 => {
                self.connection.bastion = if val.is_empty() { None } else { Some(val) };
            }
            5 => {
                self.connection.bastion_user = if val.is_empty() { None } else { Some(val) };
            }
            6 => {
                self.connection.key_path = if val.is_empty() { None } else { Some(val) };
            }
            7 => {
                self.connection.use_kerberos =
                    matches!(val.to_lowercase().as_str(), "yes" | "y" | "true" | "1");
            }
            8 => {
                self.connection.tags = val
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }

    /// Load the current field value from the connection into the buffer
    pub fn load_field(&mut self) {
        self.field_value = self.field_value_str(self.field_index);
    }

    /// Validate required fields for saving
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.connection.name.trim().is_empty() {
            return Err("Name is required");
        }
        if self.connection.host.trim().is_empty() {
            return Err("Host is required");
        }
        if self.connection.user.trim().is_empty() {
            return Err("User is required");
        }
        Ok(())
    }
}

/// The type of an active tunnel managed by the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelKind {
    /// Local port-forward (`-L`): static destination.
    Local,
    /// Dynamic SOCKS5 proxy (`-D`): destination per-request.
    Socks5,
}

impl TunnelKind {
    /// Single-character label shown in the tunnel table.
    pub fn tag(self) -> &'static str {
        match self {
            TunnelKind::Local => "L",
            TunnelKind::Socks5 => "D",
        }
    }
}

/// A live port-forward tunnel managed by the TUI.
pub struct TunnelEntry {
    /// Human-readable id (sequential, for display)
    pub id: usize,
    /// Local or SOCKS5 tunnel type
    pub kind: TunnelKind,
    /// Name of the connection being forwarded
    pub connection_name: String,
    /// Local bind address
    pub bind_host: String,
    /// Local bind port
    pub bind_port: u16,
    /// Remote target host (empty string for SOCKS5 — dynamic)
    pub remote_host: String,
    /// Remote target port (0 for SOCKS5 — dynamic)
    pub remote_port: u16,
    /// When the tunnel was started
    pub started_at: DateTime<Utc>,
    /// Owned handle — present while the tunnel is alive, consumed on cancel
    pub handle: Option<ForwardHandle>,
}

impl TunnelEntry {
    pub fn local_spec(&self) -> String {
        format!("{}:{}", self.bind_host, self.bind_port)
    }

    pub fn remote_spec(&self) -> String {
        match self.kind {
            TunnelKind::Local => format!("{}:{}", self.remote_host, self.remote_port),
            TunnelKind::Socks5 => "SOCKS5".to_string(),
        }
    }
}

/// Action to perform after TUI exits
#[derive(Debug, Clone)]
pub enum PendingAction {
    Connect,
}

/// Sort field for history tab
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistorySortField {
    Date,
    Name,
    Duration,
    Status,
}

impl HistorySortField {
    pub fn label(&self) -> &'static str {
        match self {
            HistorySortField::Date => "Date",
            HistorySortField::Name => "Name",
            HistorySortField::Duration => "Duration",
            HistorySortField::Status => "Status",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            HistorySortField::Date => HistorySortField::Name,
            HistorySortField::Name => HistorySortField::Duration,
            HistorySortField::Duration => HistorySortField::Status,
            HistorySortField::Status => HistorySortField::Date,
        }
    }
}

/// Grouping mode for connections
#[derive(Debug, Clone, PartialEq)]
pub enum GroupMode {
    None,
    ByTag,
}

/// Multi-select state
#[derive(Debug, Clone, Default)]
pub struct MultiSelectState {
    /// Indices of selected items in the filtered list
    pub selected: HashSet<usize>,
    /// Whether multi-select mode is active
    pub active: bool,
}

impl MultiSelectState {
    pub fn toggle(&mut self, index: usize) {
        if self.selected.contains(&index) {
            self.selected.remove(&index);
        } else {
            self.selected.insert(index);
        }
        self.active = !self.selected.is_empty();
    }

    pub fn select_all(&mut self, count: usize) {
        self.selected = (0..count).collect();
        self.active = count > 0;
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.active = false;
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }
}

/// Connection ping status
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable(std::time::Duration),
    Unreachable,
}

// ─── SFTP file browser ───────────────────────────────────────────────────────

/// State for the Files tab (one active SFTP browsing session).
pub struct FilesTabState {
    /// Connection being browsed
    pub connection: Connection,
    /// Remote path currently displayed
    pub current_path: String,
    /// Sorted directory listing
    pub entries: Vec<RemoteEntry>,
    /// Cursor row in the list
    pub selected: usize,
    /// True while an async SFTP operation is in-flight
    pub is_loading: bool,
    /// Error message to display in the status bar
    pub error: Option<String>,
}

impl FilesTabState {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            current_path: "/".to_string(),
            entries: Vec::new(),
            selected: 0,
            is_loading: true,
            error: None,
        }
    }

    /// Move cursor up, clamped to list bounds.
    pub fn cursor_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move cursor down, clamped to list bounds.
    pub fn cursor_down(&mut self) {
        if !self.entries.is_empty() && self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Return the currently highlighted entry, if any.
    pub fn selected_entry(&self) -> Option<&RemoteEntry> {
        self.entries.get(self.selected)
    }

    /// Compute the parent path string (returns "/" for already-root paths).
    pub fn parent_path(&self) -> String {
        let p = std::path::Path::new(&self.current_path);
        p.parent()
            .map(|par| {
                let s = par.to_string_lossy();
                if s.is_empty() { "/".to_string() } else { s.into_owned() }
            })
            .unwrap_or_else(|| "/".to_string())
    }
}

/// Messages sent from async tunnel-start tasks back to the event loop.
pub enum TunnelMsg {
    /// Tunnel started successfully.
    Started {
        kind: TunnelKind,
        connection_name: String,
        bind_host: String,
        bind_port: u16,
        remote_host: String,
        remote_port: u16,
        handle: crate::services::transport::types::ForwardHandle,
    },
    /// Tunnel failed to start.
    Failed { spec: String, error: String },
}

/// Messages sent back from async SFTP tasks to the event loop.
pub enum SftpMsg {
    /// A directory listing completed successfully.
    Listed { path: String, entries: Vec<RemoteEntry> },
    /// A file was downloaded to the given local path.
    Downloaded { remote: String, local: String, bytes: u64 },
    /// A local file was uploaded to the remote.
    Uploaded { local: String, remote: String, bytes: u64 },
    /// A remote file/dir was deleted.
    Removed { path: String },
    /// A remote directory was created.
    DirCreated { path: String },
    /// A remote entry was renamed.
    Renamed { from: String, to: String },
    /// Any SFTP error.
    Error(String),
}
