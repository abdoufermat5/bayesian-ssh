//! Files tab — SFTP remote file browser rendering

use crate::tui::models::FilesPromptKind;
use crate::tui::state::App;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

/// Draw the Files tab.
pub fn draw_files_tab(frame: &mut Frame, area: Rect, app: &App) {
    let fs = match app.files_state.as_ref() {
        Some(s) => s,
        None => {
            // Shouldn't reach this, but handle gracefully.
            let para = Paragraph::new(
                "No connection selected.\n\nPress Shift+F on a connection to open the file browser.",
            )
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
            frame.render_widget(para, area);
            return;
        }
    };

    // Split: file list (top) + hint bar (bottom, 1 row)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // ── File list ────────────────────────────────────────────────────────────
    let title = if fs.is_loading {
        format!(" Files: {}  {} [loading…] ", fs.connection.name, fs.current_path)
    } else {
        format!(" Files: {}  {} ", fs.connection.name, fs.current_path)
    };

    let border_color = if fs.error.is_some() {
        Color::Red
    } else {
        Color::Cyan
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(Color::Cyan).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));

    if fs.entries.is_empty() && !fs.is_loading {
        let msg = fs
            .error
            .as_deref()
            .unwrap_or("Directory is empty.");
        let para = Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(block);
        frame.render_widget(para, chunks[0]);
    } else {
        let items: Vec<ListItem> = fs
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let icon = if entry.is_dir {
                    "▶ "
                } else if entry.is_symlink {
                    "⇢ "
                } else {
                    "  "
                };

                let size_str = if entry.is_dir {
                    "         -".to_string()
                } else {
                    format_bytes(entry.size)
                };

                let label = format!("{}{:<40} {:>10}", icon, entry.name, size_str);

                let style = if i == fs.selected {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(30, 60, 90))
                        .bold()
                } else if entry.is_dir {
                    Style::default().fg(Color::LightBlue)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(label).style(style)
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(fs.selected));

        let list = List::new(items).block(block).highlight_symbol("► ");
        frame.render_stateful_widget(list, chunks[0], &mut list_state);
    }

    // ── Hint bar ─────────────────────────────────────────────────────────────
    let hints = Span::raw(
        " ↑/k up  ↓/j down  Enter enter  ←/h up  d dl  u upload  D delete  m mkdir  R rename  r refresh  ? help  q quit",
    );
    let hint_para = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hint_para, chunks[1]);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Draw a floating prompt dialog for upload path / mkdir name / rename.
pub fn draw_files_prompt_dialog(frame: &mut Frame, area: Rect, app: &App, kind: &FilesPromptKind) {
    let popup = centered_rect(60, 8, area);
    frame.render_widget(Clear, popup);

    let (title, hint_text) = match kind {
        FilesPromptKind::Upload => (
            " Upload File ",
            "Enter local file path:",
        ),
        FilesPromptKind::Mkdir => (
            " New Directory ",
            "Enter directory name:",
        ),
        FilesPromptKind::Rename { old_name } => (
            " Rename Entry ",
            Box::leak(format!("New name for '{old_name}':").into_boxed_str()) as &str,
        ),
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(Color::Yellow).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [hint_area, input_area, note_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(hint_text).style(Style::default().fg(Color::DarkGray)),
        hint_area,
    );

    let input_display = format!(" > {} ", app.files_prompt_input);
    frame.render_widget(
        Paragraph::new(input_display).style(Style::default().fg(Color::White).bold()),
        input_area,
    );

    frame.render_widget(
        Paragraph::new("Enter to confirm  ·  Esc to cancel")
            .style(Style::default().fg(Color::DarkGray)),
        note_area,
    );
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x + area.x, y + area.y, popup_width.min(area.width), height.min(area.height))
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1_024;
    const MB: u64 = 1_024 * KB;
    const GB: u64 = 1_024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
