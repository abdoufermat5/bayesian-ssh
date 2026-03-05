//! TUI rendering and layout

use super::app::{App, AppMode, ConfirmAction, EditState};
use crate::cli::utils::format_duration;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

/// Main draw function
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Create main layout: header, main content, status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),  // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(area);

    draw_header(frame, chunks[0], app);

    // Main content: split view when in Detail mode
    if app.mode == AppMode::Detail && !app.filtered_connections.is_empty() {
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        draw_connection_list(frame, main_chunks[0], app);
        draw_detail_pane(frame, main_chunks[1], app);
    } else {
        draw_connection_list(frame, chunks[1], app);
    }

    draw_status_bar(frame, chunks[2], app);

    // Draw overlays based on mode
    match &app.mode {
        AppMode::Help => draw_help_overlay(frame, area),
        AppMode::Confirm(action) => draw_confirm_dialog(frame, area, action, app),
        AppMode::Edit => {
            if let Some(ref edit) = app.edit_state {
                draw_edit_overlay(frame, area, edit);
            }
        }
        _ => {}
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let sort_indicator = format!(
        " [{} {}]",
        app.sort_direction.arrow(),
        app.sort_field.label()
    );

    let title = if app.mode == AppMode::Search {
        let cursor = "│";
        format!(" Search: {}{}", app.search_query, cursor)
    } else {
        format!(
            " Bayesian SSH ({}) — {} connections{}",
            app.config.environment,
            app.filtered_connections.len(),
            sort_indicator,
        )
    };

    let style = if app.mode == AppMode::Search {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Cyan).bold()
    };

    let header = Paragraph::new(title).style(style).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if app.mode == AppMode::Search {
                Color::Green
            } else {
                Color::DarkGray
            })),
    );

    frame.render_widget(header, area);
}

fn draw_connection_list(frame: &mut Frame, area: Rect, app: &App) {
    let connections = &app.filtered_connections;

    let items: Vec<ListItem> = connections
        .iter()
        .enumerate()
        .map(|(i, conn)| {
            let is_selected = i == app.selected_index;

            let bastion_indicator = if conn.bastion.is_some() {
                " [B]"
            } else {
                ""
            };
            let kerberos_indicator = if conn.use_kerberos { " [K]" } else { "" };

            if app.compact_view {
                // Compact: single line
                let tags_str = if conn.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", conn.tags.join(", "))
                };
                let line = format!(
                    "{}  {}@{}:{}{}{}{}",
                    conn.name,
                    conn.user,
                    conn.host,
                    conn.port,
                    bastion_indicator,
                    kerberos_indicator,
                    tags_str,
                );

                let style = item_style(i, is_selected);
                ListItem::new(line).style(style)
            } else {
                // Expanded: two lines
                let tags_str = if conn.tags.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", conn.tags.join(", "))
                };

                let last_used = conn
                    .last_used
                    .map(|dt| format!(" • {}", format_duration(dt)))
                    .unwrap_or_default();

                let line1 = format!(
                    "{}  {}@{}:{}{}{}",
                    conn.name,
                    conn.user,
                    conn.host,
                    conn.port,
                    bastion_indicator,
                    kerberos_indicator,
                );
                let line2 = format!("   {}{}", tags_str, last_used);

                let style = item_style(i, is_selected);
                ListItem::new(format!("{}\n{}", line1, line2)).style(style)
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
        let empty_msg = if app.search_query.is_empty() {
            "No connections found.\n\nUse 'bssh add' to create connections\nor 'bssh import' to import from ~/.ssh/config"
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
}

/// Compute item style with alternating row tint
fn item_style(index: usize, is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(30, 60, 90))
            .bold()
    } else if index.is_multiple_of(2) {
        Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(20, 20, 30))
    } else {
        Style::default().fg(Color::White)
    }
}

fn draw_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
    let conn = match app.filtered_connections.get(app.selected_index) {
        Some(c) => c,
        None => return,
    };

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("  Name:     ", Style::default().fg(Color::DarkGray)),
        Span::styled(&conn.name, Style::default().fg(Color::White).bold()),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Host:     ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}:{}", conn.host, conn.port),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  User:     ", Style::default().fg(Color::DarkGray)),
        Span::styled(&conn.user, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    if let Some(bastion) = &conn.bastion {
        let bastion_user = conn.bastion_user.as_deref().unwrap_or(&conn.user);
        lines.push(Line::from(vec![
            Span::styled("  Bastion:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}@{}", bastion_user, bastion),
                Style::default().fg(Color::Green),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Bastion:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("None", Style::default().fg(Color::DarkGray).italic()),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  Kerberos: ", Style::default().fg(Color::DarkGray)),
        if conn.use_kerberos {
            Span::styled("Enabled", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("Disabled", Style::default().fg(Color::DarkGray).italic())
        },
    ]));

    if let Some(key) = &conn.key_path {
        lines.push(Line::from(vec![
            Span::styled("  SSH Key:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(key, Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(""));

    if !conn.tags.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Tags:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                conn.tags.join(", "),
                Style::default().fg(Color::Magenta).italic(),
            ),
        ]));
    }

    if !conn.aliases.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Aliases:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(conn.aliases.join(", "), Style::default().fg(Color::Cyan)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Created:  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            conn.created_at.format("%Y-%m-%d %H:%M UTC").to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    if let Some(last_used) = conn.last_used {
        lines.push(Line::from(vec![
            Span::styled("  Last use: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_duration(last_used),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  SSH Command:",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", conn.to_ssh_command()),
        Style::default().fg(Color::Green),
    )]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Enter connect │ e edit │ s close",
        Style::default().fg(Color::DarkGray).italic(),
    )]));

    let detail = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(" Detail ")
            .title_style(Style::default().fg(Color::Yellow).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .padding(Padding::vertical(1)),
    );

    frame.render_widget(detail, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mode_str = match &app.mode {
        AppMode::Normal => "NORMAL",
        AppMode::Search => "SEARCH",
        AppMode::Help => "HELP",
        AppMode::Confirm(_) => "CONFIRM",
        AppMode::Detail => "DETAIL",
        AppMode::Edit => "EDIT",
    };

    let help_hint = match &app.mode {
        AppMode::Normal => "? help │ / search │ Enter connect │ s detail │ e edit │ d delete │ q quit",
        AppMode::Search => "Enter confirm │ Esc cancel",
        AppMode::Help => "Esc/Enter close",
        AppMode::Confirm(_) => "y confirm │ n cancel",
        AppMode::Detail => "j/k navigate │ Enter connect │ e edit │ s/Esc close",
        AppMode::Edit => "Tab next │ Shift+Tab prev │ Enter save │ Esc cancel",
    };

    // Two-section layout: left = mode + status, right = hints
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let mode_style = match &app.mode {
        AppMode::Normal => Style::default().fg(Color::Cyan).bold(),
        AppMode::Search => Style::default().fg(Color::Green).bold(),
        AppMode::Detail => Style::default().fg(Color::Yellow).bold(),
        AppMode::Edit => Style::default().fg(Color::Magenta).bold(),
        AppMode::Confirm(_) => Style::default().fg(Color::Red).bold(),
        AppMode::Help => Style::default().fg(Color::White).bold(),
    };

    let status_text = app.status_message.as_deref().unwrap_or("");

    let left = Paragraph::new(Line::from(vec![
        Span::styled(format!(" [{}] ", mode_str), mode_style),
        Span::styled(status_text, Style::default().fg(Color::Gray)),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    let right = Paragraph::new(help_hint)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Right)
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::RIGHT | Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    frame.render_widget(left, halves[0]);
    frame.render_widget(right, halves[1]);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let help_text = vec![
        "",
        "  Navigation",
        "  ──────────────────────────────────",
        "  ↑/k         Move up",
        "  ↓/j         Move down",
        "  PgUp/PgDn   Page up/down",
        "  g/Home      Go to top",
        "  G/End       Go to bottom",
        "",
        "  Actions",
        "  ──────────────────────────────────",
        "  Enter       Connect to selected",
        "  s           Show detail pane",
        "  e           Edit connection",
        "  d/Delete    Delete connection",
        "  r           Refresh list",
        "",
        "  Search & Filter",
        "  ──────────────────────────────────",
        "  /           Start search",
        "  t           Filter by tag",
        "  Esc         Clear search",
        "",
        "  View & Sort",
        "  ──────────────────────────────────",
        "  o           Cycle sort field",
        "  O           Toggle sort direction",
        "  v           Toggle compact view",
        "",
        "  General",
        "  ──────────────────────────────────",
        "  ?           Toggle this help",
        "  q/Esc       Quit",
        "",
        "  Indicators: [B] bastion  [K] kerberos",
        "",
    ];

    let help_content = help_text.join("\n");

    let popup_width = 50;
    let popup_height = help_text.len() as u16 + 2;
    let popup_area = centered_rect(popup_width, popup_height, area);

    frame.render_widget(Clear, popup_area);

    let help = Paragraph::new(help_content)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .title(" Help ")
                .title_style(Style::default().fg(Color::Yellow).bold())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    frame.render_widget(help, popup_area);
}

fn draw_confirm_dialog(frame: &mut Frame, area: Rect, action: &ConfirmAction, app: &App) {
    let (title, message) = match action {
        ConfirmAction::Delete(idx) => {
            let conn = app.filtered_connections.get(*idx);
            let name = conn.map(|c| c.name.as_str()).unwrap_or("unknown");
            (
                " Delete ",
                format!(
                    "Delete connection '{}'?\n\nThis action cannot be undone.\n\n[y] Yes  [n] No",
                    name
                ),
            )
        }
    };

    let popup_area = centered_rect(50, 10, area);

    frame.render_widget(Clear, popup_area);

    let confirm = Paragraph::new(message)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(title)
                .title_style(Style::default().fg(Color::Red).bold())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .padding(Padding::uniform(1)),
        );

    frame.render_widget(confirm, popup_area);
}

fn draw_edit_overlay(frame: &mut Frame, area: Rect, edit: &EditState) {
    let popup_width = 64;
    let popup_height = 22;
    let popup_area = centered_rect(popup_width, popup_height, area);

    frame.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for i in 0..EditState::FIELD_COUNT {
        let label = EditState::field_label(i);
        let is_active = i == edit.field_index;

        let value_str = if is_active {
            format!("{}│", edit.field_value)
        } else {
            edit.field_value_str(i)
        };

        let label_style = if is_active {
            Style::default().fg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let value_style = if is_active {
            Style::default().fg(Color::White).bg(Color::Rgb(30, 60, 90))
        } else {
            Style::default().fg(Color::White)
        };

        let arrow = if is_active { "▶ " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(format!("  {}{:<14}", arrow, format!("{}:", label)), label_style),
            Span::styled(value_str, value_style),
        ]));

        // Add a hint for kerberos field
        if i == 6 && is_active {
            lines.push(Line::from(vec![Span::styled(
                "                    (press any key to toggle)",
                Style::default().fg(Color::DarkGray).italic(),
            )]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Tab/↓ next  Shift+Tab/↑ prev  Enter save  Esc cancel",
        Style::default().fg(Color::DarkGray).italic(),
    )]));

    let edit_widget = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(format!(" Edit: {} ", edit.original_name))
            .title_style(Style::default().fg(Color::Magenta).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .padding(Padding::vertical(0)),
    );

    frame.render_widget(edit_widget, popup_area);
}

/// Helper function to create a centered rect
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
