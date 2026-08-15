use crate::tui::state::App;
use crate::tui::ui::helpers::centered_rect;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Draw the SSH command preview overlay
pub fn draw_command_preview(frame: &mut Frame, area: Rect, app: &App) {
    let conn = match app.filtered_connections.get(app.selected_index) {
        Some(c) => c,
        None => return,
    };

    let ssh_cmd = conn.to_ssh_command();

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Connection: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&conn.name, Style::default().fg(Color::White).bold()),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Command:",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!("    {}", ssh_cmd),
        Style::default().fg(Color::Green).bold(),
    )]));
    lines.push(Line::from(""));

    // Break down the components
    lines.push(Line::from(vec![Span::styled(
        "  Breakdown:",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(vec![
        Span::styled("    Host:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(&conn.host, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("    Port:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(conn.port.to_string(), Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("    User:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(&conn.user, Style::default().fg(Color::White)),
    ]));
    if let Some(bastion) = &conn.bastion {
        lines.push(Line::from(vec![
            Span::styled("    Bastion:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(bastion, Style::default().fg(Color::Green)),
        ]));
    }
    if conn.use_kerberos {
        lines.push(Line::from(vec![
            Span::styled("    Kerberos:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enabled (-K -A -t)", Style::default().fg(Color::Yellow)),
        ]));
    }
    if let Some(key) = &conn.key_path {
        lines.push(Line::from(vec![
            Span::styled("    Key:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(key, Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Enter connect | Esc/p close",
        Style::default().fg(Color::DarkGray).italic(),
    )]));

    let popup_width = 64;
    let popup_height = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup_area = centered_rect(popup_width, popup_height, area);

    frame.render_widget(Clear, popup_area);

    let preview = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(" SSH Command Preview ")
            .title_style(Style::default().fg(Color::Yellow).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(preview, popup_area);
}
