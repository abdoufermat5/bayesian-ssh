use crate::tui::models::AppMode;
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Help mode ───────────────────────────────────────────────────

    pub(crate) fn handle_help_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }
}
