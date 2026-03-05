//! UI helper functions and shared utilities

use ratatui::prelude::*;

/// Helper function to create a centered rect
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Compute item style with alternating row tint
pub fn item_style(index: usize, is_selected: bool, is_multi_selected: bool) -> Style {
    if is_selected && is_multi_selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(60, 40, 90))
            .bold()
    } else if is_selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(30, 60, 90))
            .bold()
    } else if is_multi_selected {
        Style::default().fg(Color::White).bg(Color::Rgb(40, 30, 60))
    } else if index.is_multiple_of(2) {
        Style::default().fg(Color::White).bg(Color::Rgb(20, 20, 30))
    } else {
        Style::default().fg(Color::White)
    }
}

/// Format a chrono::Duration to a human-readable short string
pub fn format_chrono_duration(d: &chrono::Duration) -> String {
    let secs = d.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
}
