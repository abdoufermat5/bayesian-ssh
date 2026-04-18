//! Header bar with tab navigation

use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Tabs},
};

/// Draw the header with tab bar
pub fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    // Split header into tab bar and info line
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2)])
        .split(area);

    // Tab bar
    let tab_titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| {
            let style = if *t == app.active_tab {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(t.label(), style))
        })
        .collect();

    let tabs = Tabs::new(tab_titles)
        .select(app.active_tab.index())
        .highlight_style(Style::default().fg(Color::Cyan).bold())
        .divider(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));

    frame.render_widget(tabs, chunks[0]);

    // Info line (context-dependent)
    let sort_indicator = format!(
        " [{} {}]",
        app.sort_direction.arrow(),
        app.sort_field.label()
    );

    let title = match app.active_tab {
        Tab::Connections => {
            if app.mode == AppMode::Search {
                let cursor = "│";
                format!(" Search: {}{}", app.search_query, cursor)
            } else if app.mode == AppMode::QuickConnect {
                let cursor = "│";
                format!(" Quick Connect: {}{}", app.quick_connect_input, cursor)
            } else {
                let multi = if app.multi_select.active {
                    format!(" ({} selected)", app.multi_select.count())
                } else {
                    String::new()
                };
                format!(
                    " {} ({}) — {} connections{}{}",
                    app.config.environment,
                    app.active_tab.label(),
                    app.filtered_connections.len(),
                    sort_indicator,
                    multi,
                )
            }
        }
        Tab::History => {
            if app.mode == AppMode::Search {
                let cursor = "│";
                format!(" Filter: {}{}", app.history_filter, cursor)
            } else {
                let filter_info = if app.history_show_failed_only {
                    " [failed only]"
                } else {
                    ""
                };
                format!(
                    " Session History — {} entries{}",
                    app.history_entries.len(),
                    filter_info,
                )
            }
        }
        Tab::Config => {
            format!(" Environments — Active: {}", app.active_env,)
        }
        Tab::Files => {
            if let Some(ref fs) = app.files_state {
                if fs.is_loading {
                    format!(" Files: {}  {} [loading…]", fs.connection.name, fs.current_path)
                } else {
                    format!(" Files: {}  {} — {} entries", fs.connection.name, fs.current_path, fs.entries.len())
                }
            } else {
                " File Browser ".to_string()
            }
        }
    };

    let style = match app.mode {
        AppMode::Search | AppMode::QuickConnect => Style::default().fg(Color::Green).bold(),
        _ => Style::default().fg(Color::Cyan).bold(),
    };

    let header = Paragraph::new(title).style(style).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(match app.mode {
                AppMode::Search | AppMode::QuickConnect => Color::Green,
                _ => Color::DarkGray,
            })),
    );

    frame.render_widget(header, chunks[1]);
}
