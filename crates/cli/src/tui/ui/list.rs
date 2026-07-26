//! Connection list rendering

use crate::cli::utils::format_duration;
use crate::tui::models::{AppMode, GroupMode, PingStatus};
use crate::tui::state::App;
use crate::tui::ui::helpers::item_style;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

/// Build a colored Span for the ping indicator
fn ping_span(status: Option<&PingStatus>) -> Span<'static> {
    match status {
        Some(PingStatus::Reachable(dur)) => {
            Span::styled(format!(" ● {:.0?}", dur), Style::default().fg(Color::Green))
        }
        Some(PingStatus::Unreachable) => Span::styled(" ●", Style::default().fg(Color::Red)),
        Some(PingStatus::Checking) => Span::styled(" ◌", Style::default().fg(Color::Yellow)),
        _ => Span::raw(""),
    }
}

/// Draw the connection list
pub fn draw_connection_list(frame: &mut Frame, area: Rect, app: &App) {
    if app.group_mode == GroupMode::ByTag {
        draw_grouped_list(frame, area, app);
        return;
    }

    let connections = &app.filtered_connections;

    let items: Vec<ListItem> = connections
        .iter()
        .enumerate()
        .map(|(i, conn)| {
            let is_selected = i == app.selected_index;
            let is_multi = app.multi_select.is_selected(i);

            let bastion_indicator = if conn.bastion.is_some() { " [B]" } else { "" };
            let kerberos_indicator = if conn.use_kerberos { " [K]" } else { "" };

            let ping = ping_span(app.ping_statuses.get(&conn.name));

            let select_marker = if is_multi { "◆ " } else { "" };
            let style = item_style(i, is_selected, is_multi);

            if app.compact_view {
                let tags_str = if conn.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", conn.tags.join(", "))
                };
                let line = Line::from(vec![
                    Span::raw(format!(
                        "{}{}  {}@{}:{}{}{}{}",
                        select_marker,
                        conn.name,
                        conn.user,
                        conn.host,
                        conn.port,
                        bastion_indicator,
                        kerberos_indicator,
                        tags_str,
                    )),
                    ping,
                ]);

                ListItem::new(line).style(style)
            } else {
                let tags_str = if conn.tags.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", conn.tags.join(", "))
                };

                let last_used = conn
                    .last_used
                    .map(|dt| format!(" • {}", format_duration(dt)))
                    .unwrap_or_default();

                let line1 = Line::from(vec![
                    Span::raw(format!(
                        "{}{}  {}@{}:{}{}{}",
                        select_marker,
                        conn.name,
                        conn.user,
                        conn.host,
                        conn.port,
                        bastion_indicator,
                        kerberos_indicator,
                    )),
                    ping,
                ]);
                let line2 = Line::from(format!("   {}{}", tags_str, last_used));

                ListItem::new(vec![line1, line2]).style(style)
            }
        })
        .collect();

    let list_title = if app.mode == AppMode::Detail {
        " Connections (s to close detail) "
    } else {
        " Connections "
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(list_title)
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
    state.select(Some(app.selected_index));

    frame.render_stateful_widget(list, area, &mut state);

    // Show empty state message
    if connections.is_empty() {
        draw_empty_state(frame, area, &app.search_query);
    }
}

fn draw_grouped_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();

    for (group_name, connections) in &app.grouped_connections {
        let collapsed = app.collapsed_groups.contains(group_name);
        let arrow = if collapsed { "▶" } else { "▼" };

        // Group header
        items.push(
            ListItem::new(format!("{} {} ({})", arrow, group_name, connections.len()))
                .style(Style::default().fg(Color::Yellow).bold()),
        );

        if !collapsed {
            for conn in connections {
                let bastion_indicator = if conn.bastion.is_some() { " [B]" } else { "" };
                let kerberos_indicator = if conn.use_kerberos { " [K]" } else { "" };
                let ping = ping_span(app.ping_statuses.get(&conn.name));
                let line = Line::from(vec![
                    Span::styled(
                        format!(
                            "    {}  {}@{}:{}{}{}",
                            conn.name,
                            conn.user,
                            conn.host,
                            conn.port,
                            bastion_indicator,
                            kerberos_indicator,
                        ),
                        Style::default().fg(Color::White),
                    ),
                    ping,
                ]);
                items.push(ListItem::new(line));
            }
        }
    }

    let list = List::new(items).block(
        Block::default()
            .title(" Connections (grouped by tag) ")
            .title_style(Style::default().fg(Color::Yellow).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .padding(Padding::horizontal(1)),
    );

    frame.render_widget(list, area);

    if app.grouped_connections.is_empty() {
        draw_empty_state(frame, area, &app.search_query);
    }
}

fn draw_empty_state(frame: &mut Frame, area: Rect, search_query: &str) {
    let empty_msg = if search_query.is_empty() {
        "No connections found.\n\nUse 'bssh add' or press 'a' to create connections\nor 'bssh import' to import from ~/.ssh/config"
    } else {
        "No matches found.\n\nTry a different search term."
    };

    let empty_paragraph = Paragraph::new(empty_msg)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 3,
    });
    frame.render_widget(empty_paragraph, inner);
}
