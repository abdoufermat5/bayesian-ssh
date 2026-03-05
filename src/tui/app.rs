//! TUI Application state and logic

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// Application mode/state
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Normal browsing mode
    Normal,
    /// Search/filter mode
    Search,
    /// Help overlay
    Help,
    /// Confirmation dialog (delete only)
    Confirm(ConfirmAction),
    /// Detail preview pane
    Detail,
    /// Inline edit mode
    Edit,
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Delete(usize),
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

/// State for inline editing
#[derive(Debug, Clone)]
pub struct EditState {
    /// Working copy of the connection being edited
    pub connection: Connection,
    /// Original connection name (for DB update lookup)
    pub original_name: String,
    /// Which field is currently selected (0-7)
    pub field_index: usize,
    /// Current input buffer for the active field
    pub field_value: String,
}

impl EditState {
    pub const FIELD_COUNT: usize = 8;

    pub fn field_label(index: usize) -> &'static str {
        match index {
            0 => "Name",
            1 => "Host",
            2 => "User",
            3 => "Port",
            4 => "Bastion",
            5 => "Bastion User",
            6 => "Kerberos",
            7 => "Tags",
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
            6 => {
                if self.connection.use_kerberos {
                    "yes".into()
                } else {
                    "no".into()
                }
            }
            7 => self.connection.tags.join(", "),
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
                self.connection.use_kerberos =
                    matches!(val.to_lowercase().as_str(), "yes" | "y" | "true" | "1");
            }
            7 => {
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
}

/// Main TUI application state
pub struct App {
    /// All connections loaded from database
    pub connections: Vec<Connection>,
    /// Filtered connections (based on search)
    pub filtered_connections: Vec<Connection>,
    /// Current search query
    pub search_query: String,
    /// Currently selected index
    pub selected_index: usize,
    /// Current application mode
    pub mode: AppMode,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Connection selected for action (after quit)
    pub selected_connection: Option<Connection>,
    /// Action to perform on selected connection
    pub pending_action: Option<PendingAction>,
    /// Status message to display
    pub status_message: Option<String>,
    /// When the status message was set (for auto-clear)
    pub status_set_at: Option<Instant>,
    /// App configuration
    pub config: AppConfig,
    /// Sort field
    pub sort_field: SortField,
    /// Sort direction
    pub sort_direction: SortDirection,
    /// Compact view mode (single-line items)
    pub compact_view: bool,
    /// Edit state (when in Edit mode)
    pub edit_state: Option<EditState>,
}

/// Action to perform after TUI exits
#[derive(Debug, Clone)]
pub enum PendingAction {
    Connect,
}

impl App {
    /// Create a new App instance
    pub fn new(config: AppConfig) -> Result<Self> {
        let db = Database::new(&config)?;
        let connections = db.list_connections(None, false)?;
        let filtered_connections = connections.clone();

        Ok(Self {
            connections,
            filtered_connections,
            search_query: String::new(),
            selected_index: 0,
            mode: AppMode::Normal,
            should_quit: false,
            selected_connection: None,
            pending_action: None,
            status_message: Some(
                "Press ? for help, / to search, Enter to connect".to_string(),
            ),
            status_set_at: Some(Instant::now()),
            config,
            sort_field: SortField::Name,
            sort_direction: SortDirection::Asc,
            compact_view: false,
            edit_state: None,
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

    /// Refresh connections from database
    pub fn refresh_connections(&mut self) -> Result<()> {
        let db = Database::new(&self.config)?;
        self.connections = db.list_connections(None, false)?;
        self.apply_filter();
        self.apply_sort();
        Ok(())
    }

    /// Apply current search filter to connections
    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_connections = self.connections.clone();
        } else if let Some(tag_query) = self.search_query.strip_prefix("tag:") {
            // Tag-specific filter
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

        // Reset selection if out of bounds
        if self.selected_index >= self.filtered_connections.len() {
            self.selected_index = self.filtered_connections.len().saturating_sub(1);
        }
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
                    // Most recent first for ascending
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

    /// Handle keyboard input
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        match &self.mode {
            AppMode::Normal => self.handle_normal_mode(key)?,
            AppMode::Search => self.handle_search_mode(key)?,
            AppMode::Help => self.handle_help_mode(key)?,
            AppMode::Confirm(action) => {
                let action = action.clone();
                self.handle_confirm_mode(key, action)?;
            }
            AppMode::Detail => self.handle_detail_mode(key)?,
            AppMode::Edit => self.handle_edit_mode(key)?,
        }
        Ok(())
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            // Navigation
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::PageUp => {
                for _ in 0..10 {
                    self.move_selection_up();
                }
            }
            KeyCode::PageDown => {
                for _ in 0..10 {
                    self.move_selection_down();
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = self.filtered_connections.len().saturating_sub(1);
            }

            // Connect directly (no confirmation)
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }

            // Delete (with confirmation)
            KeyCode::Char('d') | KeyCode::Delete => {
                if !self.filtered_connections.is_empty() {
                    self.mode = AppMode::Confirm(ConfirmAction::Delete(self.selected_index));
                }
            }

            // Detail pane
            KeyCode::Char('s') => {
                if !self.filtered_connections.is_empty() {
                    self.mode = AppMode::Detail;
                }
            }

            // Edit connection
            KeyCode::Char('e') => {
                self.enter_edit_mode();
            }

            // Search
            KeyCode::Char('/') => {
                self.mode = AppMode::Search;
                self.set_status("Type to search, Enter to confirm, Esc to cancel");
            }

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            // Refresh
            KeyCode::Char('r') => {
                self.refresh_connections()?;
                self.set_status("Connections refreshed");
            }

            // Filter by tag
            KeyCode::Char('t') => {
                self.mode = AppMode::Search;
                self.search_query = "tag:".to_string();
                self.set_status("Type tag name to filter");
            }

            // Sort: cycle field
            KeyCode::Char('o') => {
                self.sort_field = self.sort_field.next();
                self.apply_sort();
                self.set_status(format!("Sort by {}", self.sort_field.label()));
            }

            // Sort: toggle direction
            KeyCode::Char('O') => {
                self.sort_direction = self.sort_direction.toggle();
                self.apply_sort();
                self.set_status(format!(
                    "Sort {} {}",
                    self.sort_direction.arrow(),
                    self.sort_field.label()
                ));
            }

            // Compact/expanded view toggle
            KeyCode::Char('v') => {
                self.compact_view = !self.compact_view;
                let msg = if self.compact_view {
                    "Compact view"
                } else {
                    "Expanded view"
                };
                self.set_status(msg);
            }

            _ => {}
        }
        Ok(())
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.search_query.clear();
                self.apply_filter();
                self.apply_sort();
                self.set_status("Search cancelled");
            }
            KeyCode::Enter => {
                self.mode = AppMode::Normal;
                self.apply_sort();
                self.set_status(format!(
                    "Found {} connections",
                    self.filtered_connections.len()
                ));
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.apply_filter();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.apply_filter();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_help_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_detail_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => {
                self.mode = AppMode::Normal;
            }
            // Allow navigation while in detail view
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            // Connect directly from detail view
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }
            // Edit from detail view
            KeyCode::Char('e') => {
                self.enter_edit_mode();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.edit_state = None;
                self.mode = AppMode::Normal;
                self.set_status("Edit cancelled");
            }
            KeyCode::Enter => {
                // Save: apply current field, then persist
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();
                    let conn = edit.connection.clone();
                    let db = Database::new(&self.config)?;
                    db.update_connection(&conn)?;
                    self.set_status(format!("Saved connection: {}", conn.name));
                }
                self.edit_state = None;
                self.refresh_connections()?;
                self.mode = AppMode::Normal;
            }
            KeyCode::Tab | KeyCode::Down => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();
                    edit.field_index = (edit.field_index + 1) % EditState::FIELD_COUNT;
                    edit.load_field();
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();
                    edit.field_index = if edit.field_index == 0 {
                        EditState::FIELD_COUNT - 1
                    } else {
                        edit.field_index - 1
                    };
                    edit.load_field();
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.field_value.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut edit) = self.edit_state {
                    // For Kerberos field, toggle on any key press
                    if edit.field_index == 6 {
                        edit.connection.use_kerberos = !edit.connection.use_kerberos;
                        edit.load_field();
                    } else {
                        edit.field_value.push(c);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirm_mode(&mut self, key: KeyEvent, action: ConfirmAction) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match action {
                    ConfirmAction::Delete(idx) => {
                        if idx < self.filtered_connections.len() {
                            let conn = &self.filtered_connections[idx];
                            let db = Database::new(&self.config)?;
                            if db.remove_connection(&conn.name)? {
                                self.set_status(format!("Deleted connection: {}", conn.name));
                                self.refresh_connections()?;
                            }
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.set_status("Action cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    fn enter_edit_mode(&mut self) {
        if !self.filtered_connections.is_empty() {
            let conn = self.filtered_connections[self.selected_index].clone();
            let original_name = conn.name.clone();
            let mut edit = EditState {
                connection: conn,
                original_name,
                field_index: 0,
                field_value: String::new(),
            };
            edit.load_field();
            self.edit_state = Some(edit);
            self.mode = AppMode::Edit;
        }
    }

    fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    fn move_selection_down(&mut self) {
        if self.selected_index < self.filtered_connections.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Get the currently selected connection
    #[allow(dead_code)]
    pub fn get_selected_connection(&self) -> Option<&Connection> {
        self.filtered_connections.get(self.selected_index)
    }
}

/// Run the TUI event loop
pub async fn run_tui(config: AppConfig) -> Result<Option<(Connection, PendingAction)>> {
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::prelude::*;
    use std::io::stdout;

    // Suppress tracing output during TUI by redirecting stderr to /dev/null.
    // Log records written to stderr corrupt the alternate screen display.
    let saved_stderr = unsafe { libc::dup(2) };
    anyhow::ensure!(saved_stderr >= 0, "failed to dup stderr");
    {
        let devnull = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")?;
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::dup2(devnull.as_raw_fd(), 2) };
        anyhow::ensure!(rc >= 0, "failed to redirect stderr");
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(config)?;

    // Main loop
    loop {
        // Auto-clear old status messages
        app.maybe_clear_status();

        // Draw UI
        terminal.draw(|frame| {
            super::ui::draw(frame, &app);
        })?;

        // Handle events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key_event(key)?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    // Restore stderr so logging works again after TUI exits
    unsafe {
        libc::dup2(saved_stderr, 2);
        libc::close(saved_stderr);
    }

    // Return selected connection and action if any
    Ok(app.selected_connection.zip(app.pending_action))
}
