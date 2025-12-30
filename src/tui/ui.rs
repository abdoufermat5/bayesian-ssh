//! TUI rendering and layout

use super::app::{App, AppMode, ConfirmAction};
use crate::cli::utils::format_duration;
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

/// Main draw function
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Main content
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    // Draw header
    draw_header(frame, chunks[0], app);

    // Draw main content (connection list)
    draw_connection_list(frame, chunks[1], app);

    // Draw status bar
    draw_status_bar(frame, chunks[2], app);

    // Draw overlays based on mode
    match &app.mode {
        AppMode::Help => draw_help_overlay(frame, area),
        AppMode::Confirm(action) => draw_confirm_dialog(frame, area, action, app),
        _ => {}
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = if app.mode == AppMode::Search {
        format!("🔍 Search: {}_", app.search_query)
    } else {
        format!(
            "🚀 Bayesian SSH — {} connections",
            app.filtered_connections.len()
        )
    };

    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
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

            // Build connection display string
            let tags_str = if conn.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", conn.tags.join(", "))
            };

            let last_used = conn
                .last_used
                .map(|dt| format!(" • {}", format_duration(dt)))
                .unwrap_or_default();

            let bastion_indicator = if conn.bastion.is_some() { " 🔗" } else { "" };
            let kerberos_indicator = if conn.use_kerberos { " 🔐" } else { "" };

            let line1 = format!(
                "{} {}@{}:{}{}{}",
                conn.name,
                conn.user,
                conn.host,
                conn.port,
                bastion_indicator,
                kerberos_indicator
            );
            let line2 = format!("   {}{}", tags_str.trim(), last_used);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .bold()
            } else {
                Style::default().fg(Color::White)
            };

            let content = format!("{}\n{}", line1, line2);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Connections ")
                .title_style(Style::default().fg(Color::Yellow).bold())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .padding(Padding::horizontal(1)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
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

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mode_str = match &app.mode {
        AppMode::Normal => "NORMAL",
        AppMode::Search => "SEARCH",
        AppMode::Help => "HELP",
        AppMode::Confirm(_) => "CONFIRM",
    };

    let help_hint = match &app.mode {
        AppMode::Normal => "? help │ / search │ Enter connect │ d delete │ q quit",
        AppMode::Search => "Enter confirm │ Esc cancel",
        AppMode::Help => "Esc/Enter close",
        AppMode::Confirm(_) => "y confirm │ n cancel",
    };

    let status_text = app
        .status_message
        .as_deref()
        .unwrap_or(help_hint);

    let status = Paragraph::new(format!("[{}] {}", mode_str, status_text))
        .style(Style::default().fg(Color::Gray))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    frame.render_widget(status, area);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let help_text = vec![
        "",
        "  Navigation",
        "  ──────────",
        "  ↑/k       Move up",
        "  ↓/j       Move down",
        "  PgUp/PgDn Page up/down",
        "  g/Home    Go to top",
        "  G/End     Go to bottom",
        "",
        "  Actions",
        "  ───────",
        "  Enter     Connect to selected",
        "  s         Show connection details",
        "  d/Delete  Delete connection",
        "  r         Refresh list",
        "",
        "  Search",
        "  ──────",
        "  /         Start search",
        "  t         Filter by tag",
        "  Esc       Clear search",
        "",
        "  General",
        "  ───────",
        "  ?         Toggle help",
        "  q/Esc     Quit",
        "",
        "  Icons: 🔗 bastion │ 🔐 kerberos",
        "",
    ];

    let help_content = help_text.join("\n");

    // Calculate popup size
    let popup_width = 40;
    let popup_height = help_text.len() as u16 + 2;
    let popup_area = centered_rect(popup_width, popup_height, area);

    // Clear the area behind the popup
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
        ConfirmAction::Connect(idx) => {
            let conn = app.filtered_connections.get(*idx);
            let name = conn.map(|c| c.name.as_str()).unwrap_or("unknown");
            (
                " Connect ",
                format!("Connect to '{}'?\n\n[y] Yes  [n] No", name),
            )
        }
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

/// Helper function to create a centered rect
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
