//! TUI data models, enums, and small types
//!
//! Per-domain definitions:
//! - `tab`     — `Tab` (active tab in the TUI)
//! - `app_mode`— `AppMode` and related mode payloads (`ConfirmAction`, `FilesPromptKind`, `PendingAction`)
//! - `edit_state` — `EditState` (inline edit/add connection form)
//! - `sort`    — `SortField` / `SortDirection` (connection list sorting)
//! - `tunnel`  — `TunnelKind`, `TunnelEntry`, `TunnelMsg`
//! - `files`   — `FilesTabState`
//! - `sftp`    — `SftpMsg`
//! - `history` — `HistorySortField`
//! - `list`    — `GroupMode`, `MultiSelectState`, `PingStatus`

mod app_mode;
mod edit_state;
mod files;
mod history;
mod list;
mod sftp;
mod sort;
mod tab;
mod tunnel;

pub use app_mode::{AppMode, ConfirmAction, FilesPromptKind, PendingAction};
pub use edit_state::EditState;
pub use files::FilesTabState;
pub use history::HistorySortField;
pub use list::{GroupMode, MultiSelectState, PingStatus};
pub use sftp::SftpMsg;
pub use sort::{SortDirection, SortField};
pub use tab::Tab;
pub use tunnel::{TunnelEntry, TunnelKind, TunnelMsg};
