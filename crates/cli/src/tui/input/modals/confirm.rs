use crate::database::Database;
use crate::tui::models::{AppMode, ConfirmAction, Tab};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Confirm mode ────────────────────────────────────────────────

    pub(crate) fn handle_confirm_mode(
        &mut self,
        key: KeyEvent,
        action: ConfirmAction,
    ) -> Result<()> {
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
                    ConfirmAction::StopTunnel(idx) => {
                        self.stop_tunnel(idx);
                        self.set_status("Tunnel stopped");
                    }
                    ConfirmAction::DeleteFile(path) => {
                        self.files_do_delete(path);
                    }
                    ConfirmAction::QuitWithTunnels => {
                        self.should_quit = true;
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
}
