use crate::tui::models::{ConfirmAction, Tab};
use crate::tui::state::App;
use crate::tui::ui::helpers::centered_rect;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

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
        ConfirmAction::QuitWithTunnels => {
            let count = app.tunnels.len();
            (
                " Quit ",
                format!(
                    "Close {} active tunnel{}?\n\nTunnels will be cancelled on exit.\n\n[y] Yes  [n] No",
                    count,
                    if count == 1 { "" } else { "s" }
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
