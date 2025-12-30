//! TUI Application state and logic

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

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
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Delete(usize), // index of connection to delete
    Connect(usize), // index of connection to connect to
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
    /// Scroll offset for the list
    pub scroll_offset: usize,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Connection selected for action (after quit)
    pub selected_connection: Option<Connection>,
    /// Action to perform on selected connection
    pub pending_action: Option<PendingAction>,
    /// Status message to display
    pub status_message: Option<String>,
    /// App configuration
    pub config: AppConfig,
}

/// Action to perform after TUI exits
#[derive(Debug, Clone)]
pub enum PendingAction {
    Connect,
    ShowDetails,
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
            scroll_offset: 0,
            should_quit: false,
            selected_connection: None,
            pending_action: None,
            status_message: Some("Press ? for help, / to search, Enter to connect".to_string()),
            config,
        })
    }

    /// Refresh connections from database
    pub fn refresh_connections(&mut self) -> Result<()> {
        let db = Database::new(&self.config)?;
        self.connections = db.list_connections(None, false)?;
        self.apply_filter();
        Ok(())
    }

    /// Apply current search filter to connections
    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_connections = self.connections.clone();
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
        }
        Ok(())
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            // Quit with Ctrl+C
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection_down();
            }
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
                self.scroll_offset = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = self.filtered_connections.len().saturating_sub(1);
            }

            // Actions
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.mode = AppMode::Confirm(ConfirmAction::Connect(self.selected_index));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if !self.filtered_connections.is_empty() {
                    self.mode = AppMode::Confirm(ConfirmAction::Delete(self.selected_index));
                }
            }
            KeyCode::Char('s') => {
                // Show details
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::ShowDetails);
                    self.should_quit = true;
                }
            }

            // Search
            KeyCode::Char('/') => {
                self.mode = AppMode::Search;
                self.status_message = Some("Type to search, Enter to confirm, Esc to cancel".to_string());
            }

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            // Refresh
            KeyCode::Char('r') => {
                self.refresh_connections()?;
                self.status_message = Some("Connections refreshed".to_string());
            }

            // Filter by tag with 't'
            KeyCode::Char('t') => {
                self.mode = AppMode::Search;
                self.search_query = "tag:".to_string();
                self.status_message = Some("Type tag name to filter".to_string());
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
                self.status_message = Some("Search cancelled".to_string());
            }
            KeyCode::Enter => {
                self.mode = AppMode::Normal;
                self.status_message = Some(format!(
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

    fn handle_confirm_mode(&mut self, key: KeyEvent, action: ConfirmAction) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match action {
                    ConfirmAction::Connect(idx) => {
                        if idx < self.filtered_connections.len() {
                            self.selected_connection =
                                Some(self.filtered_connections[idx].clone());
                            self.pending_action = Some(PendingAction::Connect);
                            self.should_quit = true;
                        }
                    }
                    ConfirmAction::Delete(idx) => {
                        if idx < self.filtered_connections.len() {
                            let conn = &self.filtered_connections[idx];
                            let db = Database::new(&self.config)?;
                            if db.remove_connection(&conn.name)? {
                                self.status_message =
                                    Some(format!("Deleted connection: {}", conn.name));
                                self.refresh_connections()?;
                            }
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.status_message = Some("Action cancelled".to_string());
            }
            _ => {}
        }
        Ok(())
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
    pub fn get_selected_connection(&self) -> Option<&Connection> {
        self.filtered_connections.get(self.selected_index)
    }
}

/// Run the TUI event loop
pub async fn run_tui(config: AppConfig) -> Result<Option<(Connection, PendingAction)>> {
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use crossterm::execute;
    use ratatui::prelude::*;
    use std::io::stdout;

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

    // Return selected connection and action if any
    Ok(app
        .selected_connection
        .zip(app.pending_action))
}
