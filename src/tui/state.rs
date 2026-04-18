//! TUI application state management

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::connection::Connection;
use crate::models::session::SessionHistoryEntry;
use crate::services::ping;
use crate::services::transport::{pick_kind, RusshTransport, TransportKind};
use crate::services::transport::types::SshTransport;
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::models::*;

/// Main TUI application state
pub struct App {
    // -- Active tab --
    pub active_tab: Tab,

    // -- Connections tab state --
    pub connections: Vec<Connection>,
    pub filtered_connections: Vec<Connection>,
    pub search_query: String,
    pub selected_index: usize,
    pub mode: AppMode,
    pub should_quit: bool,
    pub selected_connection: Option<Connection>,
    pub pending_action: Option<PendingAction>,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub compact_view: bool,
    pub edit_state: Option<EditState>,
    pub group_mode: GroupMode,
    pub grouped_connections: Vec<(String, Vec<Connection>)>,
    pub collapsed_groups: std::collections::HashSet<String>,
    pub multi_select: MultiSelectState,
    pub quick_connect_input: String,
    pub ping_statuses: HashMap<String, PingStatus>,
    /// Sender half of the ping result channel (cloned into spawned tasks)
    pub ping_tx: mpsc::UnboundedSender<(String, PingStatus)>,
    /// Receiver half of the ping result channel (drained each event loop tick)
    pub ping_rx: mpsc::UnboundedReceiver<(String, PingStatus)>,

    // -- Files tab state --
    pub files_state: Option<FilesTabState>,
    /// Sender for SFTP async task results
    pub sftp_tx: mpsc::UnboundedSender<SftpMsg>,
    /// Receiver drained each event loop tick
    pub sftp_rx: mpsc::UnboundedReceiver<SftpMsg>,

    // -- History tab state --
    pub history_entries: Vec<SessionHistoryEntry>,
    pub history_selected: usize,
    pub history_sort_field: HistorySortField,
    pub history_filter: String,
    pub history_show_failed_only: bool,

    // -- Config tab state --
    pub env_list: Vec<String>,
    pub env_selected: usize,
    pub active_env: String,
    pub config_editing: Option<ConfigEditState>,

    // -- Shared --
    pub status_message: Option<String>,
    pub status_set_at: Option<Instant>,
    pub config: AppConfig,
}

/// Config field editing state
#[derive(Debug, Clone)]
pub struct ConfigEditState {
    pub field_value: String,
}

impl App {
    /// Create a new App instance
    pub fn new(config: AppConfig) -> Result<Self> {
        let db = Database::new(&config)?;
        let connections = db.list_connections(None, false)?;
        let filtered_connections = connections.clone();

        // Load history
        let history_entries = db
            .get_session_history(None, 100, None, false)
            .unwrap_or_default();

        // Load environments
        let (env_list, active_env) = Self::load_environments();

        // Ping result channel
        let (ping_tx, ping_rx) = mpsc::unbounded_channel();

        // SFTP result channel
        let (sftp_tx, sftp_rx) = mpsc::unbounded_channel();

        Ok(Self {
            active_tab: Tab::Connections,
            connections,
            filtered_connections,
            search_query: String::new(),
            selected_index: 0,
            mode: AppMode::Normal,
            should_quit: false,
            selected_connection: None,
            pending_action: None,
            sort_field: SortField::Name,
            sort_direction: SortDirection::Asc,
            compact_view: false,
            edit_state: None,
            group_mode: GroupMode::None,
            grouped_connections: Vec::new(),
            collapsed_groups: std::collections::HashSet::new(),
            multi_select: MultiSelectState::default(),
            quick_connect_input: String::new(),
            ping_statuses: HashMap::new(),
            ping_tx,
            ping_rx,

            files_state: None,
            sftp_tx,
            sftp_rx,

            history_entries,
            history_selected: 0,
            history_sort_field: HistorySortField::Date,
            history_filter: String::new(),
            history_show_failed_only: false,

            env_list,
            env_selected: 0,
            active_env,
            config_editing: None,

            status_message: Some("Press ? for help, / to search, Tab to switch tabs".to_string()),
            status_set_at: Some(Instant::now()),
            config,
        })
    }

    /// Set a status message with auto-clear timer
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_set_at = Some(Instant::now());
    }

    /// Clear status message if it has been shown long enough
    pub fn maybe_clear_status(&mut self) {
        if let Some(set_at) = self.status_set_at {
            if set_at.elapsed() > Duration::from_secs(3) {
                self.status_message = None;
                self.status_set_at = None;
            }
        }
    }

    /// Drain all pending ping results from the channel and update statuses.
    /// Called once per event-loop tick (non-blocking).
    pub fn drain_ping_results(&mut self) {
        while let Ok((name, status)) = self.ping_rx.try_recv() {
            // Show a brief status message for the result
            match &status {
                PingStatus::Reachable(dur) => {
                    self.set_status(format!("{}: reachable ({:.0?})", name, dur));
                }
                PingStatus::Unreachable => {
                    self.set_status(format!("{}: unreachable", name));
                }
                _ => {}
            }
            self.ping_statuses.insert(name, status);
        }
    }

    /// Spawn an async TCP ping for a connection.
    ///
    /// For connections behind a bastion, pings the bastion host on port 22.
    /// For direct connections, pings `host:port`.
    pub fn spawn_ping(&self, conn: &Connection) {
        let name = conn.name.clone();
        let (host, port) = if let Some(bastion) = &conn.bastion {
            (bastion.clone(), 22u16)
        } else {
            (conn.host.clone(), conn.port)
        };
        let tx = self.ping_tx.clone();

        tokio::spawn(async move {
            let result = ping::tcp_ping(&host, port, 5).await;
            let status = match result {
                ping::PingResult::Reachable(dur) => PingStatus::Reachable(dur),
                ping::PingResult::Unreachable => PingStatus::Unreachable,
            };
            // Ignore send errors (receiver dropped = app is shutting down)
            let _ = tx.send((name, status));
        });
    }

    // ─── SFTP file browser ───────────────────────────────────────────────────

    /// Drain completed SFTP task results and update `files_state`.
    pub fn drain_sftp_results(&mut self) {
        while let Ok(msg) = self.sftp_rx.try_recv() {
            match msg {
                SftpMsg::Listed { path, mut entries } => {
                    entries.sort_by(|a, b| {
                        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
                    });
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                        fs.error = None;
                        fs.current_path = path;
                        fs.entries = entries;
                        fs.selected = 0;
                    }
                }
                SftpMsg::Downloaded { remote, local, bytes } => {
                    self.set_status(format!(
                        "Downloaded '{}' → '{}' ({bytes} bytes)", remote, local
                    ));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                    }
                }
                SftpMsg::Error(msg) => {
                    self.set_status(format!("SFTP error: {msg}"));
                    if let Some(ref mut fs) = self.files_state {
                        fs.is_loading = false;
                        fs.error = Some(msg);
                    }
                }
            }
        }
    }

    /// Open the Files tab for `connection`, initiating a directory listing of "/".
    pub fn open_files_for_connection(&mut self, connection: Connection) {
        let state = FilesTabState::new(connection.clone());
        self.files_state = Some(state);
        self.spawn_sftp_list(connection, "/".to_string());
    }

    /// Navigate into the selected entry (if it is a directory).
    pub fn files_enter_selected(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading { return; }
            let entry = match fs.selected_entry() {
                Some(e) => e,
                None => return,
            };
            if !entry.is_dir { return; }
            let new_path = entry.path.to_string_lossy().into_owned();
            (fs.connection.clone(), new_path)
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Navigate to the parent directory.
    pub fn files_go_up(&mut self) {
        let (conn, parent) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading || fs.current_path == "/" { return; }
            (fs.connection.clone(), fs.parent_path())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, parent);
    }

    /// Refresh the current directory listing.
    pub fn files_refresh(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            (fs.connection.clone(), fs.current_path.clone())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Download the selected file to the current working directory.
    pub fn files_download_selected(&mut self) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading { return; }
        let entry = match fs.selected_entry() {
            Some(e) => e,
            None => return,
        };
        if entry.is_dir {
            self.set_status("Cannot download a directory");
            return;
        }
        let remote_path = entry.path.to_string_lossy().into_owned();
        let filename = entry.name.clone();
        let conn = fs.connection.clone();
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        tokio::spawn(async move {
            let local_path = std::path::PathBuf::from(&filename);
            let result = async {
                let kind = pick_kind(&conn, &config);
                let sftp = match kind {
                    TransportKind::Native => {
                        RusshTransport::new(config.clone())
                            .open_sftp(&conn)
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))?
                    }
                    TransportKind::Subprocess => {
                        return Err(anyhow::anyhow!(
                            "SFTP not available via subprocess transport"
                        ));
                    }
                };
                let (chunk_tx, mut chunk_rx) =
                    tokio::sync::mpsc::channel::<Vec<u8>>(16);
                let read_fut = sftp.read_all(&remote_path, chunk_tx);
                let mut file = tokio::fs::File::create(&local_path).await?;
                let write_fut = async {
                    let mut total = 0u64;
                    while let Some(chunk) = chunk_rx.recv().await {
                        use tokio::io::AsyncWriteExt;
                        file.write_all(&chunk).await?;
                        total += chunk.len() as u64;
                    }
                    Ok::<u64, anyhow::Error>(total)
                };
                let (read_result, write_result) =
                    tokio::join!(read_fut, write_fut);
                read_result.map_err(|e| anyhow::anyhow!("{e}"))?;
                write_result
            };
            match result.await {
                Ok(bytes) => {
                    let _ = tx.send(SftpMsg::Downloaded {
                        remote: remote_path,
                        local: filename,
                        bytes,
                    });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Internal: spawn a task that lists `path` on `connection` via SFTP.
    fn spawn_sftp_list(&self, conn: Connection, path: String) {
        let config = self.config.clone();
        let tx = self.sftp_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let kind = pick_kind(&conn, &config);
                match kind {
                    TransportKind::Native => {
                        RusshTransport::new(config.clone())
                            .open_sftp(&conn)
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))?
                            .list(&path)
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))
                    }
                    TransportKind::Subprocess => Err(anyhow::anyhow!(
                        "SFTP not available via subprocess transport; \
                         switch to native transport to browse files"
                    )),
                }
            };
            match result.await {
                Ok(entries) => {
                    let _ = tx.send(SftpMsg::Listed { path, entries });
                }
                Err(e) => {
                    let _ = tx.send(SftpMsg::Error(e.to_string()));
                }
            }
        });
    }

    /// Refresh connections from database
    pub fn refresh_connections(&mut self) -> Result<()> {
        let db = Database::new(&self.config)?;
        self.connections = db.list_connections(None, false)?;
        self.apply_filter();
        self.apply_sort();
        if self.group_mode != GroupMode::None {
            self.build_groups();
        }
        Ok(())
    }

    /// Refresh history from database
    pub fn refresh_history(&mut self) -> Result<()> {
        let db = Database::new(&self.config)?;
        let filter = if self.history_filter.is_empty() {
            None
        } else {
            Some(self.history_filter.as_str())
        };
        self.history_entries =
            db.get_session_history(filter, 100, None, self.history_show_failed_only)?;
        if self.history_selected >= self.history_entries.len() {
            self.history_selected = self.history_entries.len().saturating_sub(1);
        }
        Ok(())
    }

    /// Load environment list
    fn load_environments() -> (Vec<String>, String) {
        let active = AppConfig::get_active_env();
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
            .join("bayesian-ssh")
            .join("environments");

        let mut envs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&config_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        envs.push(name.to_string());
                    }
                }
            }
        }
        envs.sort();
        if envs.is_empty() {
            envs.push("default".to_string());
        }
        (envs, active)
    }

    /// Refresh environment list
    pub fn refresh_environments(&mut self) {
        let (envs, active) = Self::load_environments();
        self.env_list = envs;
        self.active_env = active;
        if self.env_selected >= self.env_list.len() {
            self.env_selected = self.env_list.len().saturating_sub(1);
        }
    }

    /// Apply current search filter to connections
    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_connections = self.connections.clone();
        } else if let Some(tag_query) = self.search_query.strip_prefix("tag:") {
            let tag_q = tag_query.to_lowercase();
            if tag_q.is_empty() {
                self.filtered_connections = self.connections.clone();
            } else {
                self.filtered_connections = self
                    .connections
                    .iter()
                    .filter(|c| c.tags.iter().any(|t| t.to_lowercase().contains(&tag_q)))
                    .cloned()
                    .collect();
            }
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_connections = self
                .connections
                .iter()
                .filter(|c| {
                    c.name.to_lowercase().contains(&query)
                        || c.host.to_lowercase().contains(&query)
                        || c.user.to_lowercase().contains(&query)
                        || c.tags.iter().any(|t| t.to_lowercase().contains(&query))
                })
                .cloned()
                .collect();
        }

        if self.selected_index >= self.filtered_connections.len() {
            self.selected_index = self.filtered_connections.len().saturating_sub(1);
        }

        // Clear multi-select when filter changes
        self.multi_select.clear();
    }

    /// Sort filtered connections based on current sort settings
    pub fn apply_sort(&mut self) {
        let dir = self.sort_direction;
        match self.sort_field {
            SortField::Name => {
                self.filtered_connections.sort_by(|a, b| {
                    let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    if dir == SortDirection::Desc {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            SortField::Host => {
                self.filtered_connections.sort_by(|a, b| {
                    let cmp = a.host.to_lowercase().cmp(&b.host.to_lowercase());
                    if dir == SortDirection::Desc {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            SortField::LastUsed => {
                self.filtered_connections.sort_by(|a, b| {
                    let cmp = a.last_used.cmp(&b.last_used);
                    if dir == SortDirection::Asc {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            SortField::Created => {
                self.filtered_connections.sort_by(|a, b| {
                    let cmp = a.created_at.cmp(&b.created_at);
                    if dir == SortDirection::Desc {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
        }

        if self.selected_index >= self.filtered_connections.len() {
            self.selected_index = self.filtered_connections.len().saturating_sub(1);
        }
    }

    /// Build grouped connection list by tags
    pub fn build_groups(&mut self) {
        let mut groups: HashMap<String, Vec<Connection>> = HashMap::new();
        let mut ungrouped = Vec::new();

        for conn in &self.filtered_connections {
            if conn.tags.is_empty() {
                ungrouped.push(conn.clone());
            } else {
                for tag in &conn.tags {
                    groups.entry(tag.clone()).or_default().push(conn.clone());
                }
            }
        }

        let mut result: Vec<(String, Vec<Connection>)> = groups.into_iter().collect();
        result.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        if !ungrouped.is_empty() {
            result.push(("Ungrouped".to_string(), ungrouped));
        }

        self.grouped_connections = result;
    }

    /// Move selection up in the current tab
    pub fn move_selection_up(&mut self) {
        match self.active_tab {
            Tab::Connections => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
            Tab::History => {
                if self.history_selected > 0 {
                    self.history_selected -= 1;
                }
            }
            Tab::Config => {
                if self.env_selected > 0 {
                    self.env_selected -= 1;
                }
            }
            Tab::Files => {
                if let Some(ref mut fs) = self.files_state {
                    fs.cursor_up();
                }
            }
        }
    }

    /// Move selection down in the current tab
    pub fn move_selection_down(&mut self) {
        match self.active_tab {
            Tab::Connections => {
                if self.selected_index < self.filtered_connections.len().saturating_sub(1) {
                    self.selected_index += 1;
                }
            }
            Tab::History => {
                if self.history_selected < self.history_entries.len().saturating_sub(1) {
                    self.history_selected += 1;
                }
            }
            Tab::Config => {
                if self.env_selected < self.env_list.len().saturating_sub(1) {
                    self.env_selected += 1;
                }
            }
            Tab::Files => {
                if let Some(ref mut fs) = self.files_state {
                    fs.cursor_down();
                }
            }
        }
    }

    /// Get the currently selected connection
    #[allow(dead_code)]
    pub fn get_selected_connection(&self) -> Option<&Connection> {
        self.filtered_connections.get(self.selected_index)
    }
}
