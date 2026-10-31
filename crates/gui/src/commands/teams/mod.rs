//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.
//!
//! `#[tauri::command]` functions are re-exported with a glob so the
//! `__cmd__*` macros Tauri generates next to them stay reachable.

mod billing;
mod dto;
mod identity;
mod org;
mod state;
mod team;

pub use billing::*;
pub use identity::*;
pub use org::*;
pub use state::TeamsState;
pub use team::*;
