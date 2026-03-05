//! Connection detail pane rendering

use crate::cli::utils::format_duration;
use crate::tui::state::App;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Padding, Paragraph},
};

/// Draw the detail pane for the selected connection
pub fn draw_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
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
        "  Enter connect | e edit | p preview | s close",
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
