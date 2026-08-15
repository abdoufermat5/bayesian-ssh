use crate::models::Connection;
use crate::services::transport::types::RemoteEntry;

/// State for the Files tab (one active SFTP browsing session).
pub struct FilesTabState {
    /// Connection being browsed
    pub connection: Connection,
    /// Remote path currently displayed
    pub current_path: String,
    /// Sorted directory listing
    pub entries: Vec<RemoteEntry>,
    /// Cursor row in the list
    pub selected: usize,
    /// True while an async SFTP operation is in-flight
    pub is_loading: bool,
    /// Error message to display in the status bar
    pub error: Option<String>,
}

impl FilesTabState {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            current_path: "/".to_string(),
            entries: Vec::new(),
            selected: 0,
            is_loading: true,
            error: None,
        }
    }

    /// Move cursor up, clamped to list bounds.
    pub fn cursor_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move cursor down, clamped to list bounds.
    pub fn cursor_down(&mut self) {
        if !self.entries.is_empty() && self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Return the currently highlighted entry, if any.
    pub fn selected_entry(&self) -> Option<&RemoteEntry> {
        self.entries.get(self.selected)
    }

    /// Compute the parent path string (returns "/" for already-root paths).
    pub fn parent_path(&self) -> String {
        let p = std::path::Path::new(&self.current_path);
        p.parent()
            .map(|par| {
                let s = par.to_string_lossy();
                if s.is_empty() {
                    "/".to_string()
                } else {
                    s.into_owned()
                }
            })
            .unwrap_or_else(|| "/".to_string())
    }
}
