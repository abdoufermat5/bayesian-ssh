//! Config/Environment tab rendering

use crate::tui::state::App;
use crate::tui::ui::helpers::item_style;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};

/// Draw the config/environment tab
pub fn draw_config_tab(frame: &mut Frame, area: Rect, app: &App) {
    // Split into environment list and config details
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    draw_env_list(frame, chunks[0], app);
    draw_config_details(frame, chunks[1], app);
}

fn draw_env_list(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .env_list
        .iter()
        .enumerate()
        .map(|(i, env)| {
            let is_selected = i == app.env_selected;
            let is_active = *env == app.active_env;

            let marker = if is_active { " * " } else { "   " };
            let label = format!("{}{}", marker, env);

            let mut style = item_style(i, is_selected, false);
            if is_active && !is_selected {
                style = style.fg(Color::Green).bold();
            }

            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Environments ")
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
    state.select(Some(app.env_selected));

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_config_details(frame: &mut Frame, area: Rect, app: &App) {
    let config = &app.config;

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Environment:   ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &config.environment,
            Style::default().fg(Color::Green).bold(),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Connection Defaults",
        Style::default().fg(Color::Cyan).bold(),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "  ────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  Default User:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(&config.default_user, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Default Port:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            config.default_port.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Default Bastion: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            config.default_bastion.as_deref().unwrap_or("None"),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Kerberos:        ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if config.use_kerberos_by_default {
                "Enabled"
            } else {
                "Disabled"
            },
            Style::default().fg(if config.use_kerberos_by_default {
                Color::Yellow
            } else {
                Color::White
            }),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Search & Storage",
        Style::default().fg(Color::Cyan).bold(),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "  ────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  Search Mode:     ", Style::default().fg(Color::DarkGray)),
        Span::styled(&config.search_mode, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Log Level:       ", Style::default().fg(Color::DarkGray)),
        Span::styled(&config.log_level, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Auto Save:       ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if config.auto_save_history {
                "Yes"
            } else {
                "No"
            },
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Max History:     ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            config.max_history_size.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Database",
        Style::default().fg(Color::Cyan).bold(),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "  ────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )]));
    lines.push(Line::from(vec![
        Span::styled("  DB Path:         ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            config.database_path.display().to_string(),
            Style::default().fg(Color::DarkGray).italic(),
        ),
    ]));

    let detail = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(" Configuration ")
            .title_style(Style::default().fg(Color::Yellow).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .padding(Padding::vertical(0)),
    );

    frame.render_widget(detail, area);
}
