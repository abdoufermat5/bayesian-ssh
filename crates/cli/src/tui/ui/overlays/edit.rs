use crate::tui::models::{AppMode, EditState};
use crate::tui::ui::helpers::centered_rect;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

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
