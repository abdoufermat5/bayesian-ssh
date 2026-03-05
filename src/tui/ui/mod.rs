//! TUI rendering — main draw dispatcher
//!
//! Routes rendering to the appropriate tab view and overlays
//! based on the current application state.

pub mod config;
pub mod detail;
pub mod header;
pub mod helpers;
pub mod history;
pub mod list;
pub mod overlays;
pub mod status;

use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use ratatui::prelude::*;

/// Main draw function called by the event loop each frame.
///
/// Layout:
/// ┌──────────────────────────────────┐
/// │  Header (tab bar + info line)    │  3 rows
/// ├──────────────────────────────────┤
/// │                                  │
/// │  Body (tab content)              │  remaining
/// │                                  │
/// ├──────────────────────────────────┤
/// │  Status bar                      │  3 rows
/// └──────────────────────────────────┘
pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Top-level vertical layout: header, body, status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header (tab bar + info line)
            Constraint::Min(1),    // body
            Constraint::Length(3), // status bar
        ])
        .split(size);

    // ── Header ──────────────────────────────────────────────────
    header::draw_header(frame, chunks[0], app);

    // ── Body (tab-dependent) ────────────────────────────────────
    draw_body(frame, chunks[1], app);

    // ── Status bar ──────────────────────────────────────────────
    status::draw_status_bar(frame, chunks[2], app);

    // ── Overlays (drawn on top of everything) ───────────────────
    draw_overlays(frame, size, app);
}

/// Draw the main body area based on the active tab and mode.
fn draw_body(frame: &mut Frame, area: Rect, app: &App) {
    match app.active_tab {
        Tab::Connections => draw_connections_body(frame, area, app),
        Tab::History => history::draw_history_tab(frame, area, app),
        Tab::Config => config::draw_config_tab(frame, area, app),
    }
}

/// Draw the connections tab body.
///
/// In Detail mode the area is split into list (left) and detail pane (right).
/// Otherwise the connection list takes the full width.
fn draw_connections_body(frame: &mut Frame, area: Rect, app: &App) {
    if app.mode == AppMode::Detail {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        list::draw_connection_list(frame, chunks[0], app);
        detail::draw_detail_pane(frame, chunks[1], app);
    } else {
        list::draw_connection_list(frame, area, app);
    }
}

/// Draw modal overlays on top of the existing UI.
fn draw_overlays(frame: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        AppMode::Help => {
            overlays::draw_help_overlay(frame, area, app);
        }
        AppMode::Confirm(action) => {
            overlays::draw_confirm_dialog(frame, area, action, app);
        }
        AppMode::Edit | AppMode::Add => {
            // Connection edit/add overlay (connections tab)
            if app.active_tab == Tab::Connections {
                if let Some(ref edit) = app.edit_state {
                    overlays::draw_edit_overlay(frame, area, edit, &app.mode);
                }
            }
            // Config tab environment creation dialog
            if app.active_tab == Tab::Config {
                overlays::draw_config_edit_dialog(frame, area, app);
            }
        }
        AppMode::CommandPreview => {
            overlays::draw_command_preview(frame, area, app);
        }
        // Normal, Search, Detail, QuickConnect — no overlay needed
        // (Search & QuickConnect are shown inline in the header)
        _ => {}
    }
}
