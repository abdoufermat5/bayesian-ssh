//! Status bar rendering

use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draw the status bar
pub fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mode_str = match &app.mode {
        AppMode::Normal => "NORMAL",
        AppMode::Search => "SEARCH",
        AppMode::Help => "HELP",
        AppMode::Confirm(_) => "CONFIRM",
        AppMode::Detail => "DETAIL",
        AppMode::Edit => "EDIT",
        AppMode::Add => "ADD",
        AppMode::QuickConnect => "QUICK",
        AppMode::CommandPreview => "PREVIEW",
    };

    let help_hint = match (&app.mode, app.active_tab) {
        (AppMode::Normal, Tab::Connections) => {
            if app.multi_select.active {
                "x batch delete | Esc clear | Space toggle"
            } else {
                "? help | / search | a add | : quick | Enter connect | q quit"
            }
        }
        (AppMode::Normal, Tab::History) => {
            "/ filter | f toggle failed | o sort | Enter reconnect | q quit"
        }
        (AppMode::Normal, Tab::Config) => {
            "Enter switch env | a create | d delete | r refresh | q quit"
        }
        (AppMode::Normal, Tab::Files) => {
            "↑/k up | ↓/j down | Enter enter dir | ←/h up | d download | r refresh | q quit"
        }
        (AppMode::Search, _) => "Enter confirm | Esc cancel",
        (AppMode::Help, _) => "Esc/Enter close",
        (AppMode::Confirm(_), _) => "y confirm | n cancel",
        (AppMode::Detail, _) => "j/k navigate | Enter connect | e edit | p preview | s/Esc close",
        (AppMode::Edit, _) | (AppMode::Add, _) => {
            "Tab next | Shift+Tab prev | Enter save | Esc cancel"
        }
        (AppMode::QuickConnect, _) => "[user@]host[:port] | Enter connect | Esc cancel",
        (AppMode::CommandPreview, _) => "Enter connect | Esc close",
    };

    // Two-section layout
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let mode_style = match &app.mode {
        AppMode::Normal => Style::default().fg(Color::Cyan).bold(),
        AppMode::Search | AppMode::QuickConnect => Style::default().fg(Color::Green).bold(),
        AppMode::Detail | AppMode::CommandPreview => Style::default().fg(Color::Yellow).bold(),
        AppMode::Edit | AppMode::Add => Style::default().fg(Color::Magenta).bold(),
        AppMode::Confirm(_) => Style::default().fg(Color::Red).bold(),
        AppMode::Help => Style::default().fg(Color::White).bold(),
    };

    let status_text = app.status_message.as_deref().unwrap_or("");

    let tab_hint = "1/2/3 tabs | ".to_string();

    let left = Paragraph::new(Line::from(vec![
        Span::styled(format!(" [{}] ", mode_str), mode_style),
        Span::styled(status_text, Style::default().fg(Color::Gray)),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    let right = Paragraph::new(Line::from(vec![
        Span::styled(tab_hint, Style::default().fg(Color::DarkGray)),
        Span::styled(help_hint, Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(Alignment::Right)
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(left, halves[0]);
    frame.render_widget(right, halves[1]);
}
