//! PTY sessions: spawning, output flow control, detach/reattach, pop-out
//! windows, teardown and the clipboard.
//!
//! `#[tauri::command]` functions are re-exported with a glob so the
//! `__cmd__*` macros Tauri generates next to them stay reachable from
//! `generate_handler!`.

mod clipboard;
mod detach;
mod lifecycle;
mod output;
mod popout;
mod replay;
mod spawn;

pub use clipboard::*;
pub use detach::*;
pub use lifecycle::*;
pub use output::OutputSink;
pub use popout::*;
pub use spawn::*;
