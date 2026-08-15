use crate::config::AppConfig;
use crate::models::Connection;
use crate::tui::models::{AppMode, PendingAction};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Quick connect mode ──────────────────────────────────────────

    pub(crate) fn handle_quick_connect_mode(&mut self, key: KeyEvent) -> Result<()> {
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
