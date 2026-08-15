use crate::tui::models::{AppMode, PendingAction};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Command preview mode ────────────────────────────────────────

    pub(crate) fn handle_command_preview_mode(&mut self, key: KeyEvent) -> Result<()> {
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
}
