/// Application mode/state
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Normal browsing mode
    Normal,
    /// Search/filter mode
    Search,
    /// Help overlay
    Help,
    /// Confirmation dialog
    Confirm(ConfirmAction),
    /// Detail preview pane
    Detail,
    /// Inline edit mode
    Edit,
    /// Add new connection mode
    Add,
    /// Quick connect bar
    QuickConnect,
    /// SSH command preview
    CommandPreview,
    /// Tunnel launch dialog (entering -L spec)
    TunnelLaunch,
    /// Files tab prompt dialog (upload path / mkdir name / rename target)
    FilesPrompt(FilesPromptKind),
}

/// What the Files tab prompt dialog is collecting
#[derive(Debug, Clone, PartialEq)]
pub enum FilesPromptKind {
    /// Upload: user types a local file path
    Upload,
    /// Download: user types a local destination path
    Download { remote_path: String, is_dir: bool },
    /// Mkdir: user types a directory name (relative to current path)
    Mkdir,
    /// Rename: user types the new name; old_name is the current file name
    Rename { old_name: String },
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Delete(usize),
    BatchDelete,
    StopTunnel(usize),
    /// Delete a remote file/dir at the given path
    DeleteFile(String),
    /// Quit the TUI while one or more tunnels are still active
    QuitWithTunnels,
}

/// Action to perform after TUI exits
#[derive(Debug, Clone)]
pub enum PendingAction {
    Connect,
}
