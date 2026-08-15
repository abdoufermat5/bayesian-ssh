use crate::database::Database;
use crate::models::Connection;
use crate::tui::models::{AppMode, EditState, Tab};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Edit / Add mode ─────────────────────────────────────────────

    pub(crate) fn handle_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
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
}
