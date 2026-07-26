//! TUI keyboard input handling

use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

mod modals;
mod tabs;

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
                KeyCode::Char('4') => {
                    // Switch to Files tab only if a connection is already loaded
                    if self.files_state.is_some() {
                        self.switch_to_tab(Tab::Files);
                    } else {
                        self.set_status("Press 'f' on a connection to open the file browser");
                    }
                    return Ok(());
                }
                KeyCode::Char('5') => {
                    self.switch_to_tab(Tab::Tunnels);
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
                Tab::Files => self.handle_files_normal(key)?,
                Tab::Tunnels => self.handle_tunnels_normal(key)?,
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
            AppMode::TunnelLaunch => self.handle_tunnel_launch_mode(key)?,
            AppMode::FilesPrompt(kind) => {
                let kind = kind.clone();
                self.handle_files_prompt_mode(key, kind)?;
            }
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
            Tab::Files => {
                if let Some(ref fs) = self.files_state {
                    self.set_status(format!("Files: {}", fs.connection.name));
                }
            }
            Tab::Tunnels => {
                self.set_status(format!("Tunnels ({} active)", self.tunnels.len()));
            }
        }
    }
}
