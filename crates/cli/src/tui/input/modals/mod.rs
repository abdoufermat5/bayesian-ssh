//! TUI modal input handling
//!
//! One module per modal/dialog type:
//! - `tunnel`        — TunnelLaunch dialog (`-L` / SOCKS5 `-D` specs)
//! - `files`         — Files tab prompt dialog (upload / download / mkdir / rename)
//! - `edit`          — Connection edit/add mode
//! - `config`        — Config tab environment creation dialog
//! - `confirm`       — Confirmation dialog (delete / stop tunnel / quit with tunnels)
//! - `quick_connect` — Quick connect bar
//! - `search`        — Search/filter mode
//! - `help`          — Help overlay mode
//! - `detail`        — Detail preview pane mode
//! - `command_preview` — SSH command preview mode

mod command_preview;
mod config;
mod confirm;
mod detail;
mod edit;
mod files;
mod help;
mod quick_connect;
mod search;
mod tunnel;
