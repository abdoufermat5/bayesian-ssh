//! Overlay dialogs: help, confirm, edit/add, command preview
//!
//! One renderer per overlay:
//! - `help`           — `draw_help_overlay`
//! - `confirm`        — `draw_confirm_dialog`
//! - `edit`           — `draw_edit_overlay` (connection edit/add)
//! - `config`         — `draw_config_edit_dialog` (environment creation)
//! - `command_preview`— `draw_command_preview`

mod command_preview;
mod config;
mod confirm;
mod edit;
mod help;

pub use command_preview::draw_command_preview;
pub use config::draw_config_edit_dialog;
pub use confirm::draw_confirm_dialog;
pub use edit::draw_edit_overlay;
pub use help::draw_help_overlay;
