use crate::database::Database;
use crate::tui::models::{
    AppMode, ConfirmAction, FilesPromptKind, GroupMode, PendingAction, PingStatus, Tab,
};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    // ─── Connections tab: Normal mode ────────────────────────────────

    pub(crate) fn handle_connections_normal(&mut self, key: KeyEvent) -> Result<()> {
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

            // Open Files tab for the highlighted connection (Shift+F)
            KeyCode::Char('F') => {
                if let Some(conn) = self.filtered_connections.get(self.selected_index).cloned() {
                    let name = conn.name.clone();
                    self.set_status(format!("Opening file browser for {}...", name));
                    self.open_files_for_connection(conn);
                    self.switch_to_tab(Tab::Files);
                }
            }

            _ => {}
        }
        Ok(())
    }

    // ─── History tab: Normal mode ────────────────────────────────────

    pub(crate) fn handle_history_normal(&mut self, key: KeyEvent) -> Result<()> {
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

    pub(crate) fn handle_config_normal(&mut self, key: KeyEvent) -> Result<()> {
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
                        if let Ok(()) = crate::config::AppConfig::set_active_env(&env_name) {
                            self.active_env = env_name.clone();
                            // Reload config for new environment
                            if let Ok(new_config) =
                                crate::config::AppConfig::load(Some(env_name.clone()))
                            {
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
                self.config_editing = Some(crate::tui::state::ConfigEditState {
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

    // ─── Files tab: Normal mode ──────────────────────────────────────

    pub(crate) fn handle_files_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ref mut fs) = self.files_state {
                    fs.cursor_down();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ref mut fs) = self.files_state {
                    fs.cursor_up();
                }
            }
            KeyCode::Enter => {
                self.files_enter_selected();
            }
            KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
                self.files_go_up();
            }
            KeyCode::Char('r') => {
                self.files_refresh();
                self.set_status("Refreshing...");
            }
            KeyCode::Char('d') => {
                // Open download prompt with pre-filled local path
                if let Some(ref fs) = self.files_state {
                    if let Some(entry) = fs.selected_entry() {
                        let remote_path = entry.path.to_string_lossy().into_owned();
                        let is_dir = entry.is_dir;
                        self.files_prompt_input = entry.name.clone();
                        self.mode = AppMode::FilesPrompt(FilesPromptKind::Download {
                            remote_path,
                            is_dir,
                        });
                    }
                }
            }
            KeyCode::Char('u') => {
                self.files_prompt_input.clear();
                self.mode = AppMode::FilesPrompt(FilesPromptKind::Upload);
            }
            KeyCode::Char('D') => {
                self.files_delete_selected();
            }
            KeyCode::Char('m') => {
                self.files_prompt_input.clear();
                self.mode = AppMode::FilesPrompt(FilesPromptKind::Mkdir);
            }
            KeyCode::Char('R') => {
                if let Some(old_name) = self
                    .files_state
                    .as_ref()
                    .and_then(|fs| fs.selected_entry())
                    .map(|e| e.name.clone())
                {
                    self.files_prompt_input.clear();
                    self.mode = AppMode::FilesPrompt(FilesPromptKind::Rename { old_name });
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Tunnels tab: Normal mode ────────────────────────────────────

    pub(crate) fn handle_tunnels_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.tunnels.is_empty() {
                    self.tunnel_selected = (self.tunnel_selected + 1).min(self.tunnels.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.tunnel_selected = self.tunnel_selected.saturating_sub(1);
            }
            KeyCode::Char('n') => {
                // Pick a connection then open TunnelLaunch dialog
                self.open_tunnel_launch();
            }
            KeyCode::Char('d') => {
                // Open SOCKS5 dynamic proxy dialog
                self.open_socks5_launch();
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if !self.tunnels.is_empty() {
                    let idx = self.tunnel_selected;
                    self.mode = AppMode::Confirm(ConfirmAction::StopTunnel(idx));
                }
            }
            _ => {}
        }
        Ok(())
    }
}
