use crate::config::AppConfig;
use crate::tui::models::AppMode;
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub(crate) fn handle_config_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
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
}
