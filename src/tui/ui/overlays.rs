//! Overlay dialogs: help, confirm, edit/add, command preview

use crate::tui::models::{AppMode, ConfirmAction, EditState, Tab};
use crate::tui::state::App;
use crate::tui::ui::helpers::centered_rect;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

/// Draw the help overlay
pub fn draw_help_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.active_tab {
        Tab::Connections => vec![
            "",
            "  Navigation",
            "  ──────────────────────────────────────",
            "  1/2/3       Switch tab",
            "  Tab         Next tab",
            "  ↑/k         Move up",
            "  ↓/j         Move down",
            "  PgUp/PgDn   Page up/down",
            "  g/Home      Go to top",
            "  G/End       Go to bottom",
            "",
            "  Actions",
            "  ──────────────────────────────────────",
            "  Enter       Connect to selected",
            "  a           Add new connection",
            "  s           Show detail pane",
            "  e           Edit connection",
            "  p           Preview SSH command",
            "  d/Delete    Delete connection",
            "  r           Refresh list",
            "  :           Quick connect",
            "",
            "  Selection",
            "  ──────────────────────────────────────",
            "  Space       Toggle select",
            "  Ctrl+A      Select all",
            "  x           Batch delete selected",
            "  Esc         Clear selection",
            "",
            "  Search & Filter",
            "  ──────────────────────────────────────",
            "  /           Start search",
            "  t           Filter by tag",
            "",
            "  View & Sort",
            "  ──────────────────────────────────────",
            "  o           Cycle sort field",
            "  O           Toggle sort direction",
            "  v           Toggle compact view",
            "  f           Toggle group by tag",
            "",
            "  General",
            "  ──────────────────────────────────────",
            "  ?           Toggle this help",
            "  P           Ping selected host",
            "  q/Esc       Quit",
            "",
            "  Indicators: [B] bastion  [K] kerberos",
            "  Ping:  ● reachable  ● unreachable  ◌ checking",
            "",
        ],
        Tab::History => vec![
            "",
            "  Navigation",
            "  ──────────────────────────────────────",
            "  1/2/3       Switch tab",
            "  ↑/k ↓/j     Move up/down",
            "  PgUp/PgDn   Page up/down",
            "",
            "  Actions",
            "  ──────────────────────────────────────",
            "  Enter       Reconnect to session host",
            "  r           Refresh history",
            "  /           Filter by connection name",
            "  f           Toggle show failed only",
            "  o           Cycle sort field",
            "",
            "  General",
            "  ──────────────────────────────────────",
            "  ?           Toggle this help",
            "  q/Esc       Quit",
            "",
        ],
        Tab::Config => vec![
            "",
            "  Navigation",
            "  ──────────────────────────────────────",
            "  1/2/3       Switch tab",
            "  ↑/k ↓/j     Move up/down",
            "",
            "  Actions",
            "  ──────────────────────────────────────",
            "  Enter       Switch to environment",
            "  a           Create new environment",
            "  d/Delete    Delete environment",
            "  r           Refresh list",
            "",
            "  General",
            "  ──────────────────────────────────────",
            "  ?           Toggle this help",
            "  q/Esc       Quit",
            "",
        ],
        Tab::Files => vec![
            "",
            "  Navigation",
            "  ──────────────────────────────────────",
            "  ↑/k ↓/j     Move up/down",
            "  Enter       Enter directory",
            "  ←/h/BS      Go to parent directory",
            "",
            "  Actions",
            "  ──────────────────────────────────────",
            "  d           Download selected file",
            "  u           Upload local file",
            "  D           Delete selected entry",
            "  m           New directory (mkdir)",
            "  R           Rename selected entry",
            "  r           Refresh listing",
            "",
            "  General",
            "  ──────────────────────────────────────",
            "  ?           Toggle this help",
            "  q/Esc       Quit",
            "",
        ],
        Tab::Tunnels => vec![
            "",
            "  Navigation",
            "  ──────────────────────────────────────",
            "  ↑/k ↓/j     Move up/down",
            "  1-5         Switch tab",
            "",
            "  Actions",
            "  ──────────────────────────────────────",
            "  n           New -L port-forward tunnel",
            "  d           New SOCKS5 dynamic proxy (-D)",
            "  x/Delete    Stop selected tunnel",
            "",
            "  Tunnel spec format  (-L)",
            "  ──────────────────────────────────────",
            "  bind_port:remote_host:remote_port",
            "  bind_addr:bind_port:remote_host:remote_port",
            "",
            "  Proxy spec format  (-D)",
            "  ──────────────────────────────────────",
            "  port",
            "  bind_addr:port",
            "",
            "  General",
            "  ──────────────────────────────────────",
            "  ?           Toggle this help",
            "  q/Esc       Quit",
            "",
        ],
    };

    let help_content = help_text.join("\n");

    let popup_width = 54;
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

/// Draw the confirm dialog
pub fn draw_confirm_dialog(frame: &mut Frame, area: Rect, action: &ConfirmAction, app: &App) {
    let (title, message) = match action {
        ConfirmAction::Delete(idx) => {
            if app.active_tab == Tab::Config {
                let env_name = app
                    .env_list
                    .get(*idx)
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                (
                    " Delete Environment ",
                    format!(
                        "Delete environment '{}'?\n\nAll connections and history in this\nenvironment will be lost.\n\n[y] Yes  [n] No",
                        env_name
                    ),
                )
            } else {
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
        }
        ConfirmAction::BatchDelete => {
            let count = app.multi_select.count();
            (
                " Batch Delete ",
                format!(
                    "Delete {} selected connections?\n\nThis action cannot be undone.\n\n[y] Yes  [n] No",
                    count
                ),
            )
        }
        ConfirmAction::StopTunnel(idx) => {
            let spec = app
                .tunnels
                .get(*idx)
                .map(|t| {
                    format!(
                        "{}:{} → {}:{}",
                        t.bind_host, t.bind_port, t.remote_host, t.remote_port
                    )
                })
                .unwrap_or_else(|| "unknown".to_string());
            (
                " Stop Tunnel ",
                format!("Stop tunnel {}?\n\n[y] Yes  [n] No", spec),
            )
        }
        ConfirmAction::DeleteFile(path) => (
            " Delete Remote Entry ",
            format!("Delete '{path}'?\n\nThis cannot be undone.\n\n[y] Yes  [n] No"),
        ),
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

/// Draw the edit/add overlay
pub fn draw_edit_overlay(frame: &mut Frame, area: Rect, edit: &EditState, mode: &AppMode) {
    let popup_width = 64;
    let popup_height = 24;
    let popup_area = centered_rect(popup_width, popup_height, area);

    frame.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for i in 0..EditState::FIELD_COUNT {
        let label = EditState::field_label(i);
        let is_active = i == edit.field_index;

        let value_str = if is_active {
            format!("{}|", edit.field_value)
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
            Span::styled(
                format!("  {}{:<14}", arrow, format!("{}:", label)),
                label_style,
            ),
            Span::styled(value_str, value_style),
        ]));

        // Add hint for kerberos field
        if i == 7 && is_active {
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

    let title = if *mode == AppMode::Add {
        " New Connection "
    } else {
        &format!(" Edit: {} ", edit.original_name)
    };

    let border_color = if *mode == AppMode::Add {
        Color::Green
    } else {
        Color::Magenta
    };

    let title_style = Style::default().fg(border_color).bold();

    let edit_widget = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(title.to_string())
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .padding(Padding::vertical(0)),
    );

    frame.render_widget(edit_widget, popup_area);
}

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

/// Draw config tab environment creation dialog
pub fn draw_config_edit_dialog(frame: &mut Frame, area: Rect, app: &App) {
    if let Some(ref edit) = app.config_editing {
        let popup_area = centered_rect(50, 7, area);
        frame.render_widget(Clear, popup_area);

        let content = format!("\n  Name: {}|", edit.field_value);

        let dialog = Paragraph::new(content)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(" Create Environment ")
                    .title_style(Style::default().fg(Color::Green).bold())
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .padding(Padding::horizontal(1)),
            );

        frame.render_widget(dialog, popup_area);
    }
}
