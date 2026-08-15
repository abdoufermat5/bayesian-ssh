use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Search mode ───────────────────────────────────────────────────────────────

    pub(crate) fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
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
}
