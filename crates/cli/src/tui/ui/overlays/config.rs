use crate::tui::state::App;
use crate::tui::ui::helpers::centered_rect;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

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
