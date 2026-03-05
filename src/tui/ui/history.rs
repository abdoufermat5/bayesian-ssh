//! History tab rendering

use crate::models::session::SessionStatus;
use crate::tui::state::App;
use crate::tui::ui::helpers::{format_chrono_duration, item_style};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

/// Draw the history tab
pub fn draw_history_tab(frame: &mut Frame, area: Rect, app: &App) {
    let entries = &app.history_entries;

    if entries.is_empty() {
        let block = Block::default()
            .title(" Session History ")
            .title_style(Style::default().fg(Color::Yellow).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        frame.render_widget(block, area);

        let msg = if app.history_filter.is_empty() && !app.history_show_failed_only {
            "No session history found.\n\nConnect to a server to start building history."
        } else {
            "No matching sessions found.\n\nTry adjusting the filter."
        };

        let empty = Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        let inner = area.inner(Margin {
            horizontal: 2,
            vertical: 3,
        });
        frame.render_widget(empty, inner);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == app.history_selected;

            let status_str = match &entry.status {
                SessionStatus::Terminated => {
                    match entry.exit_code {
                        Some(0) => ("OK", Color::Green),
                        Some(code) => {
                            // We'll just show the exit code inline
                            return ListItem::new(format!(
                                "  {:<20} {:<20} {:>8}   exit:{}",
                                entry.connection_name,
                                entry.started_at.format("%Y-%m-%d %H:%M"),
                                entry
                                    .duration
                                    .as_ref()
                                    .map(format_chrono_duration)
                                    .unwrap_or_else(|| "-".to_string()),
                                code,
                            ))
                            .style(item_style(i, is_selected, false));
                        }
                        None => ("DONE", Color::Green),
                    }
                }
                SessionStatus::Active => ("ACTIVE", Color::Cyan),
                SessionStatus::Starting => ("START", Color::Yellow),
                SessionStatus::Disconnected => ("DISC", Color::Yellow),
                SessionStatus::Error(_) => ("ERROR", Color::Red),
            };

            let duration_str = entry
                .duration
                .as_ref()
                .map(format_chrono_duration)
                .unwrap_or_else(|| "-".to_string());

            let line = format!(
                "  {:<20} {:<20} {:>8}   {}",
                entry.connection_name,
                entry.started_at.format("%Y-%m-%d %H:%M"),
                duration_str,
                status_str.0,
            );

            let mut style = item_style(i, is_selected, false);
            // Tint the status color
            if !is_selected {
                style = style.fg(status_str.1);
            }

            ListItem::new(line).style(style)
        })
        .collect();

    // Column header
    let header_line = format!(
        "  {:<20} {:<20} {:>8}   {}",
        "Connection", "Date", "Duration", "Status"
    );

    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    let header = Paragraph::new(header_line).style(Style::default().fg(Color::DarkGray).bold());

    frame.render_widget(header, inner_area[0]);

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Session History ")
                .title_style(Style::default().fg(Color::Yellow).bold())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .padding(Padding::horizontal(1)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 60, 90))
                .bold(),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.history_selected));

    frame.render_stateful_widget(list, inner_area[1], &mut state);
}
