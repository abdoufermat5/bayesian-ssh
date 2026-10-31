//! System clipboard and PRIMARY selection commands.

use crate::commands::CommandError;
use std::sync::Mutex;

/// The system clipboard, opened once and kept for the app's lifetime. On X11
/// and Wayland the process that copied must stay alive to serve the data, so a
/// handle dropped right after a write would lose it.
pub(super) static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

pub(super) fn with_clipboard<T>(
    f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, CommandError> {
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().map_err(|e| CommandError::from(e.to_string()))?);
    }
    let clipboard = guard.as_mut().expect("clipboard initialised above");
    f(clipboard).map_err(|e| CommandError::from(e.to_string()))
}

/// Read the system clipboard as text.
///
/// The terminal uses this instead of `navigator.clipboard`, which WebKit (the
/// macOS and Linux webviews) only allows behind a paste prompt or not at all.
/// `None` when the clipboard is empty, holds no text, or is unavailable.
#[tauri::command(async)]
pub fn read_clipboard_text() -> Option<String> {
    with_clipboard(|clipboard| clipboard.get_text()).ok()
}

/// Put text on the system clipboard (see [`read_clipboard_text`]).
#[tauri::command(async)]
pub fn write_clipboard_text(text: String) -> Result<(), CommandError> {
    with_clipboard(|clipboard| clipboard.set_text(text))
}

/// Read the PRIMARY selection on Linux, where middle-click paste uses it.
#[cfg(target_os = "linux")]
pub(super) fn primary_selection() -> Option<String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    with_clipboard(|clipboard| {
        clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
    })
    .ok()
}

/// Only X11 and Wayland have a PRIMARY selection; elsewhere middle-click paste
/// falls back to the default clipboard in the frontend.
#[cfg(not(target_os = "linux"))]
pub(super) fn primary_selection() -> Option<String> {
    None
}

/// Read the PRIMARY selection, the X11/Wayland buffer middle-click paste uses.
///
/// Runs off the main thread because a selection owner that does not answer can
/// make the clipboard backend block for a while. `None` when no owner answers
/// or the backend is unavailable (headless session, no display server).
#[tauri::command(async)]
pub fn read_primary_selection() -> Option<String> {
    primary_selection()
}
