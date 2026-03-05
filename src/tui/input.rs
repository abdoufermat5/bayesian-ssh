//! TUI keyboard input handling

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::models::*;
use super::state::{App, ConfigEditState};

impl App {
    /// Handle keyboard input - top-level dispatcher
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        // Global keys first (tab switching)
        if self.mode == AppMode::Normal {
            match key.code {
                KeyCode::Char('1') => {
                    self.switch_to_tab(Tab::Connections);
                    return Ok(());
                }
                KeyCode::Char('2') => {
                    self.switch_to_tab(Tab::History);
                    return Ok(());
                }
                KeyCode::Char('3') => {
                    self.switch_to_tab(Tab::Config);
                    return Ok(());
                }
                KeyCode::Tab if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    let next = self.active_tab.next();
                    self.switch_to_tab(next);
                    return Ok(());
                }
                KeyCode::BackTab => {
                    let prev = self.active_tab.prev();
                    self.switch_to_tab(prev);
                    return Ok(());
                }
                _ => {}
            }
        }

        // Mode-specific handling
        match &self.mode {
            AppMode::Normal => match self.active_tab {
                Tab::Connections => self.handle_connections_normal(key)?,
                Tab::History => self.handle_history_normal(key)?,
                Tab::Config => self.handle_config_normal(key)?,
            },
            AppMode::Search => self.handle_search_mode(key)?,
            AppMode::Help => self.handle_help_mode(key)?,
            AppMode::Confirm(action) => {
                let action = action.clone();
                self.handle_confirm_mode(key, action)?;
            }
            AppMode::Detail => self.handle_detail_mode(key)?,
            AppMode::Edit => self.handle_edit_mode(key)?,
            AppMode::Add => self.handle_edit_mode(key)?,
            AppMode::QuickConnect => self.handle_quick_connect_mode(key)?,
            AppMode::CommandPreview => self.handle_command_preview_mode(key)?,
        }
        Ok(())
    }

    fn switch_to_tab(&mut self, tab: Tab) {
        if self.active_tab == tab {
            return;
        }
        self.active_tab = tab;
        self.mode = AppMode::Normal;
        match tab {
            Tab::Connections => {
                let _ = self.refresh_connections();
                self.set_status("Connections");
            }
            Tab::History => {
                let _ = self.refresh_history();
                self.set_status("Session History");
            }
            Tab::Config => {
                self.refresh_environments();
                self.set_status("Environment Configuration");
            }
        }
    }

    // ─── Connections tab: Normal mode ────────────────────────────────

    fn handle_connections_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.multi_select.active {
                    self.multi_select.clear();
                    self.set_status("Selection cleared");
                } else {
                    self.should_quit = true;
                }
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

            // Connect
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }

            // Select all (Ctrl+A) — must be before plain 'a'
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.multi_select
                    .select_all(self.filtered_connections.len());
                self.set_status(format!("{} selected", self.multi_select.count()));
            }

            // Add new connection
            KeyCode::Char('a') => {
                self.enter_add_mode();
            }

            // Delete (with confirmation)
            KeyCode::Char('d') | KeyCode::Delete => {
                if self.multi_select.active {
                    self.mode = AppMode::Confirm(ConfirmAction::BatchDelete);
                } else if !self.filtered_connections.is_empty() {
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

            // SSH command preview
            KeyCode::Char('p') => {
                if !self.filtered_connections.is_empty() {
                    self.mode = AppMode::CommandPreview;
                }
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

            KeyCode::Char('f') => {
                self.group_mode = match self.group_mode {
                    GroupMode::None => {
                        self.build_groups();
                        self.set_status("Grouped by tag");
                        GroupMode::ByTag
                    }
                    GroupMode::ByTag => {
                        self.set_status("Ungrouped");
                        GroupMode::None
                    }
                };
            }

            // Multi-select toggle
            KeyCode::Char(' ') => {
                self.multi_select.toggle(self.selected_index);
                let count = self.multi_select.count();
                if count > 0 {
                    self.set_status(format!("{} selected (x batch delete, Esc clear)", count));
                } else {
                    self.set_status("Selection cleared");
                }
            }

            // Batch delete
            KeyCode::Char('x') => {
                if self.multi_select.active {
                    self.mode = AppMode::Confirm(ConfirmAction::BatchDelete);
                }
            }

            // Quick connect
            KeyCode::Char(':') => {
                self.quick_connect_input.clear();
                self.mode = AppMode::QuickConnect;
                self.set_status("Quick connect: [user@]host[:port]");
            }

            // Ping selected connection
            KeyCode::Char('P') => {
                if let Some(conn) = self.filtered_connections.get(self.selected_index).cloned() {
                    let name = conn.name.clone();
                    self.ping_statuses
                        .insert(name.clone(), PingStatus::Checking);
                    self.set_status(format!("Pinging {}...", name));
                    self.spawn_ping(&conn);
                }
            }

            _ => {}
        }
        Ok(())
    }

    // ─── History tab: Normal mode ────────────────────────────────────

    fn handle_history_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
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
                self.history_selected = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.history_selected = self.history_entries.len().saturating_sub(1);
            }

            // Reconnect: find connection by name and connect
            KeyCode::Enter => {
                if let Some(entry) = self.history_entries.get(self.history_selected) {
                    let db = Database::new(&self.config)?;
                    if let Some(conn) = db.get_connection(&entry.connection_name)? {
                        self.selected_connection = Some(conn);
                        self.pending_action = Some(PendingAction::Connect);
                        self.should_quit = true;
                    } else {
                        self.set_status(format!(
                            "Connection '{}' no longer exists",
                            entry.connection_name
                        ));
                    }
                }
            }

            // Refresh
            KeyCode::Char('r') => {
                self.refresh_history()?;
                self.set_status("History refreshed");
            }

            // Toggle failed only
            KeyCode::Char('f') => {
                self.history_show_failed_only = !self.history_show_failed_only;
                self.refresh_history()?;
                if self.history_show_failed_only {
                    self.set_status("Showing failed sessions only");
                } else {
                    self.set_status("Showing all sessions");
                }
            }

            // Sort
            KeyCode::Char('o') => {
                self.history_sort_field = self.history_sort_field.next();
                self.set_status(format!("Sort by {}", self.history_sort_field.label()));
            }

            // Search
            KeyCode::Char('/') => {
                self.mode = AppMode::Search;
                self.set_status("Filter history by connection name");
            }

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            _ => {}
        }
        Ok(())
    }

    // ─── Config tab: Normal mode ─────────────────────────────────────

    fn handle_config_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),

            // Switch environment
            KeyCode::Enter => {
                if let Some(env_name) = self.env_list.get(self.env_selected).cloned() {
                    if env_name != self.active_env {
                        if let Ok(()) = AppConfig::set_active_env(&env_name) {
                            self.active_env = env_name.clone();
                            // Reload config for new environment
                            if let Ok(new_config) = AppConfig::load(Some(env_name.clone())) {
                                self.config = new_config;
                                let _ = self.refresh_connections();
                                let _ = self.refresh_history();
                            }
                            self.set_status(format!("Switched to environment: {}", env_name));
                        }
                    }
                }
            }

            // Create environment
            KeyCode::Char('a') => {
                self.config_editing = Some(ConfigEditState {
                    field_value: String::new(),
                });
                self.mode = AppMode::Edit;
                self.set_status("Enter new environment name");
            }

            // Delete environment
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(env_name) = self.env_list.get(self.env_selected) {
                    if env_name == "default" {
                        self.set_status("Cannot delete the default environment");
                    } else if *env_name == self.active_env {
                        self.set_status("Cannot delete the active environment");
                    } else {
                        self.mode = AppMode::Confirm(ConfirmAction::Delete(self.env_selected));
                    }
                }
            }

            // Refresh
            KeyCode::Char('r') => {
                self.refresh_environments();
                self.set_status("Environments refreshed");
            }

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            _ => {}
        }
        Ok(())
    }

    // ─── Search mode ─────────────────────────────────────────────────

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                if self.active_tab == Tab::Connections {
                    self.search_query.clear();
                    self.apply_filter();
                    self.apply_sort();
                } else if self.active_tab == Tab::History {
                    self.history_filter.clear();
                    self.refresh_history()?;
                }
                self.set_status("Search cancelled");
            }
            KeyCode::Enter => {
                self.mode = AppMode::Normal;
                if self.active_tab == Tab::Connections {
                    self.apply_sort();
                    self.set_status(format!(
                        "Found {} connections",
                        self.filtered_connections.len()
                    ));
                } else if self.active_tab == Tab::History {
                    self.refresh_history()?;
                    self.set_status(format!("Found {} entries", self.history_entries.len()));
                }
            }
            KeyCode::Backspace => {
                if self.active_tab == Tab::Connections {
                    self.search_query.pop();
                    self.apply_filter();
                } else if self.active_tab == Tab::History {
                    self.history_filter.pop();
                    let _ = self.refresh_history();
                }
            }
            KeyCode::Char(c) => {
                if self.active_tab == Tab::Connections {
                    self.search_query.push(c);
                    self.apply_filter();
                } else if self.active_tab == Tab::History {
                    self.history_filter.push(c);
                    let _ = self.refresh_history();
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Help mode ───────────────────────────────────────────────────

    fn handle_help_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Detail mode ─────────────────────────────────────────────────

    fn handle_detail_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }
            KeyCode::Char('e') => {
                self.enter_edit_mode();
            }
            KeyCode::Char('p') => {
                self.mode = AppMode::CommandPreview;
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Edit / Add mode ─────────────────────────────────────────────

    fn handle_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
        // Handle config tab editing (environment creation) separately
        if self.active_tab == Tab::Config {
            return self.handle_config_edit_mode(key);
        }

        match key.code {
            KeyCode::Esc => {
                self.edit_state = None;
                self.mode = AppMode::Normal;
                self.set_status("Edit cancelled");
            }
            KeyCode::Enter => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();

                    // Validate before saving
                    if let Err(msg) = edit.validate() {
                        self.set_status(msg);
                        return Ok(());
                    }

                    let conn = edit.connection.clone();
                    let is_new = edit.is_new;
                    let db = Database::new(&self.config)?;

                    if is_new {
                        db.add_connection(&conn)?;
                        self.set_status(format!("Created connection: {}", conn.name));
                    } else {
                        db.update_connection(&conn)?;
                        self.set_status(format!("Saved connection: {}", conn.name));
                    }
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
                    if edit.field_index == 7 {
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

    fn handle_config_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.config_editing = None;
                self.mode = AppMode::Normal;
                self.set_status("Cancelled");
            }
            KeyCode::Enter => {
                if let Some(ref edit) = self.config_editing {
                    let name = edit.field_value.trim().to_string();
                    if !name.is_empty() {
                        let env_dir = dirs::config_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                            .join("bayesian-ssh")
                            .join("environments")
                            .join(&name);

                        if env_dir.exists() {
                            self.set_status(format!("Environment '{}' already exists", name));
                        } else {
                            std::fs::create_dir_all(&env_dir)?;
                            let config = AppConfig::default_for_env(&name);
                            config.save()?;
                            self.set_status(format!("Created environment: {}", name));
                            self.refresh_environments();
                        }
                    }
                }
                self.config_editing = None;
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                if let Some(ref mut edit) = self.config_editing {
                    edit.field_value.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut edit) = self.config_editing {
                    edit.field_value.push(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Confirm mode ────────────────────────────────────────────────

    fn handle_confirm_mode(&mut self, key: KeyEvent, action: ConfirmAction) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match action {
                    ConfirmAction::Delete(idx) => {
                        if self.active_tab == Tab::Config {
                            // Delete environment
                            if let Some(env_name) = self.env_list.get(idx).cloned() {
                                let env_dir = dirs::config_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                                    .join("bayesian-ssh")
                                    .join("environments")
                                    .join(&env_name);
                                if env_dir.exists() {
                                    std::fs::remove_dir_all(&env_dir)?;
                                    self.set_status(format!("Deleted environment: {}", env_name));
                                    self.refresh_environments();
                                }
                            }
                        } else if idx < self.filtered_connections.len() {
                            let conn = &self.filtered_connections[idx];
                            let db = Database::new(&self.config)?;
                            if db.remove_connection(&conn.name)? {
                                self.set_status(format!("Deleted connection: {}", conn.name));
                                self.refresh_connections()?;
                            }
                        }
                    }
                    ConfirmAction::BatchDelete => {
                        let db = Database::new(&self.config)?;
                        let mut deleted = 0;
                        // Sort indices in reverse to avoid shifting issues
                        let mut indices: Vec<usize> =
                            self.multi_select.selected.iter().copied().collect();
                        indices.sort_unstable_by(|a, b| b.cmp(a));
                        for idx in indices {
                            if idx < self.filtered_connections.len() {
                                let conn = &self.filtered_connections[idx];
                                if db.remove_connection(&conn.name)? {
                                    deleted += 1;
                                }
                            }
                        }
                        self.multi_select.clear();
                        self.refresh_connections()?;
                        self.set_status(format!("Deleted {} connections", deleted));
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

    // ─── Quick connect mode ──────────────────────────────────────────

    fn handle_quick_connect_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.quick_connect_input.clear();
                self.mode = AppMode::Normal;
                self.set_status("Quick connect cancelled");
            }
            KeyCode::Enter => {
                let input = self.quick_connect_input.trim().to_string();
                if !input.is_empty() {
                    if let Some(conn) = Self::parse_quick_connect(&input, &self.config) {
                        self.selected_connection = Some(conn);
                        self.pending_action = Some(PendingAction::Connect);
                        self.should_quit = true;
                    } else {
                        self.set_status("Invalid format. Use: [user@]host[:port]");
                    }
                }
                if !self.should_quit {
                    self.quick_connect_input.clear();
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Backspace => {
                self.quick_connect_input.pop();
            }
            KeyCode::Char(c) => {
                self.quick_connect_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Command preview mode ────────────────────────────────────────

    fn handle_command_preview_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('p') | KeyCode::Char('q') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                // Connect directly from preview
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Helpers ─────────────────────────────────────────────────────

    pub fn enter_edit_mode(&mut self) {
        if !self.filtered_connections.is_empty() {
            let conn = self.filtered_connections[self.selected_index].clone();
            let original_name = conn.name.clone();
            let mut edit = EditState {
                connection: conn,
                original_name,
                is_new: false,
                field_index: 0,
                field_value: String::new(),
            };
            edit.load_field();
            self.edit_state = Some(edit);
            self.mode = AppMode::Edit;
        }
    }

    pub fn enter_add_mode(&mut self) {
        let conn = Connection::new(
            String::new(),
            String::new(),
            self.config.default_user.clone(),
            self.config.default_port,
            self.config.default_bastion.clone(),
            self.config.default_bastion_user.clone(),
            self.config.use_kerberos_by_default,
            None,
        );
        let mut edit = EditState {
            connection: conn,
            original_name: String::new(),
            is_new: true,
            field_index: 0,
            field_value: String::new(),
        };
        edit.load_field();
        self.edit_state = Some(edit);
        self.mode = AppMode::Add;
    }

    /// Parse a quick connect string: [user@]host[:port]
    fn parse_quick_connect(input: &str, config: &AppConfig) -> Option<Connection> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        let (user, rest) = if let Some(at_pos) = input.find('@') {
            let user = &input[..at_pos];
            let rest = &input[at_pos + 1..];
            (user.to_string(), rest)
        } else {
            (config.default_user.clone(), input)
        };

        let (host, port) = if let Some(colon_pos) = rest.rfind(':') {
            let host = &rest[..colon_pos];
            let port_str = &rest[colon_pos + 1..];
            match port_str.parse::<u16>() {
                Ok(p) => (host.to_string(), p),
                Err(_) => (rest.to_string(), config.default_port),
            }
        } else {
            (rest.to_string(), config.default_port)
        };

        if host.is_empty() {
            return None;
        }

        let name = format!("quick-{}", host);
        Some(Connection::new(
            name, host, user, port, None, None, false, None,
        ))
    }
}
