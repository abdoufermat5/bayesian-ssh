//! SFTP orchestration state for the Files tab.
//!
//! Navigation and confirmation live here; the async task spawning and
//! result draining machinery lives in `super::sftp_tasks`.

use crate::models::Connection;
use crate::tui::models::{AppMode, ConfirmAction, FilesTabState};
use crate::tui::state::App;

impl App {
    /// Open the Files tab for `connection`, initiating a directory listing of "/".
    pub fn open_files_for_connection(&mut self, connection: Connection) {
        let state = FilesTabState::new(connection.clone());
        self.files_state = Some(state);
        self.spawn_sftp_list(connection, "/".to_string());
    }

    /// Navigate into the selected entry (if it is a directory).
    pub fn files_enter_selected(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading {
                return;
            }
            let entry = match fs.selected_entry() {
                Some(e) => e,
                None => return,
            };
            if !entry.is_dir {
                return;
            }
            let new_path = entry.path.to_string_lossy().into_owned();
            (fs.connection.clone(), new_path)
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Navigate to the parent directory.
    pub fn files_go_up(&mut self) {
        let (conn, parent) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            if fs.is_loading || fs.current_path == "/" {
                return;
            }
            (fs.connection.clone(), fs.parent_path())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, parent);
    }

    /// Refresh the current directory listing.
    pub fn files_refresh(&mut self) {
        let (conn, path) = {
            let fs = match self.files_state.as_ref() {
                Some(s) => s,
                None => return,
            };
            (fs.connection.clone(), fs.current_path.clone())
        };
        if let Some(ref mut fs) = self.files_state {
            fs.is_loading = true;
        }
        self.spawn_sftp_list(conn, path);
    }

    /// Delete the selected remote entry (file or empty directory).
    pub fn files_delete_selected(&mut self) {
        let fs = match self.files_state.as_ref() {
            Some(s) => s,
            None => return,
        };
        if fs.is_loading {
            return;
        }
        let entry = match fs.selected_entry() {
            Some(e) => e,
            None => return,
        };
        let path = entry.path.to_string_lossy().into_owned();
        self.mode = AppMode::Confirm(ConfirmAction::DeleteFile(path));
    }
}
