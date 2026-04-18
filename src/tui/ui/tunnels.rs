//! Tunnels tab rendering — active port-forward tunnel management

use crate::tui::state::App;
use crate::tui::ui::helpers::{centered_rect, item_style};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

/// Draw the Tunnels tab.
pub fn draw_tunnels_tab(frame: &mut Frame, area: Rect, app: &App) {
    if app.tunnels.is_empty() {
        draw_empty_state(frame, area);
    } else {
        draw_tunnel_list(frame, area, app);
    }
}

fn draw_empty_state(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Active Tunnels ")
        .title_style(Style::default().fg(Color::Yellow).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::uniform(1));

    let msg = Paragraph::new(
        "No active tunnels.\n\n\
         Press  n  to start a new tunnel from a connection.\n\
         Press  ?  for help.",
    )
    .style(Style::default().fg(Color::DarkGray))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true })
    .block(block);

    frame.render_widget(msg, area);
}

fn draw_tunnel_list(frame: &mut Frame, area: Rect, app: &App) {
    // Header line inside the block
    let header = format!(
        "  {:<4}  {:<20}  {:<22}  {:<22}  {}",
        "#", "Connection", "Local (bind)", "Remote target", "Uptime"
    );

    let items: Vec<ListItem> = app
        .tunnels
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_selected = i == app.tunnel_selected;
            let uptime = {
                let elapsed = chrono::Utc::now()
                    .signed_duration_since(t.started_at)
                    .to_std()
                    .unwrap_or_default();
                format_uptime(elapsed)
            };

            let line = format!(
                "  {:<4}  {:<20}  {:<22}  {:<22}  {}",
                t.id,
                truncate(&t.connection_name, 20),
                truncate(&t.local_spec(), 22),
                truncate(&t.remote_spec(), 22),
                uptime,
            );

            ListItem::new(line).style(item_style(i, is_selected, false))
        })
        .collect();

    let block = Block::default()
        .title(format!(" Active Tunnels ({}) ", app.tunnels.len()))
        .title_style(Style::default().fg(Color::Yellow).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .padding(Padding::new(0, 0, 0, 0));

    // Split area: header row + list
    let [header_area, list_area] = Layout::vertical([
        Constraint::Length(2), // column headers
        Constraint::Min(1),
    ])
    .areas(area.inner(Margin { horizontal: 0, vertical: 0 }));

    let header_widget = Paragraph::new(header)
        .style(Style::default().fg(Color::DarkGray).bold())
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(Color::Blue))
            .title(" Active Tunnels ")
            .title_style(Style::default().fg(Color::Yellow).bold()));

    frame.render_widget(header_widget, header_area);

    let mut list_state = ListState::default().with_selected(Some(app.tunnel_selected));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .bold(),
        );
    frame.render_stateful_widget(list, list_area, &mut list_state);

    let _ = block; // consumed above
}

/// Draw the TunnelLaunch input dialog (modal overlay).
pub fn draw_tunnel_launch_dialog(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(60, 10, area);
    frame.render_widget(Clear, popup);

    let target_name = app
        .tunnel_target
        .as_ref()
        .map(|c| c.name.as_str())
        .unwrap_or("<none>");

    let title = format!(" New tunnel → {} ", target_name);
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(Color::Cyan).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [hint_area, input_area, note_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new("Enter -L spec:  [bind_addr:]bind_port:remote_host:remote_port")
            .style(Style::default().fg(Color::DarkGray)),
        hint_area,
    );

    let input_display = format!(" > {} ", app.tunnel_input);
    frame.render_widget(
        Paragraph::new(input_display).style(Style::default().fg(Color::White).bold()),
        input_area,
    );

    frame.render_widget(
        Paragraph::new("Enter to start  ·  Esc to cancel")
            .style(Style::default().fg(Color::DarkGray)),
        note_area,
    );
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn format_uptime(d: std::time::Duration) -> String {
    let total = d.as_secs();
    if total < 60 {
        format!("{}s", total)
    } else if total < 3600 {
        format!("{}m {}s", total / 60, total % 60)
    } else {
        format!("{}h {}m", total / 3600, (total % 3600) / 60)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
