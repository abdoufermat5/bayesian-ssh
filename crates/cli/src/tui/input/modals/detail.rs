use crate::tui::models::{AppMode, PendingAction};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Detail mode ─────────────────────────────────────────────────

    pub(crate) fn handle_detail_mode(&mut self, key: KeyEvent) -> Result<()> {
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
}
