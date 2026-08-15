use crate::tui::models::{AppMode, FilesPromptKind};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    // ─── Files prompt mode (upload / mkdir / rename) ─────────────────

    pub(crate) fn handle_files_prompt_mode(
        &mut self,
        key: KeyEvent,
        kind: FilesPromptKind,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.files_prompt_input.clear();
                self.set_status("Cancelled");
            }
            KeyCode::Enter => {
                let input = self.files_prompt_input.trim().to_string();
                if input.is_empty() {
                    self.set_status("Please enter a value first");
                    return Ok(());
                }
                self.mode = AppMode::Normal;
                self.files_prompt_input.clear();
                match kind {
                    FilesPromptKind::Upload => {
                        let local = std::path::Path::new(&input);
                        if local.is_dir() {
                            self.set_status(format!("Uploading directory '{input}'…"));
                            self.files_upload_dir(input);
                        } else {
                            self.set_status(format!("Uploading '{input}'…"));
                            self.files_upload(input);
                        }
                    }
                    FilesPromptKind::Download {
                        remote_path,
                        is_dir,
                    } => {
                        if is_dir {
                            self.set_status(format!("Downloading directory → '{input}'…"));
                            self.files_download_dir_to(remote_path, input);
                        } else {
                            self.set_status(format!("Downloading → '{input}'…"));
                            self.files_download_to(remote_path, input);
                        }
                    }
                    FilesPromptKind::Mkdir => {
                        self.set_status(format!("Creating directory '{input}'…"));
                        self.files_mkdir(input);
                    }
                    FilesPromptKind::Rename { .. } => {
                        self.set_status(format!("Renaming → '{input}'…"));
                        self.files_rename(input);
                    }
                }
            }
            KeyCode::Backspace => {
                self.files_prompt_input.pop();
            }
            KeyCode::Char(c) => {
                self.files_prompt_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }
}
