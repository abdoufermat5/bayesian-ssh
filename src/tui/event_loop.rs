//! TUI event loop and terminal setup/teardown

use crate::config::AppConfig;
use crate::models::Connection;
use anyhow::Result;
use crossterm::event::{self, Event};
use std::time::Duration;

use super::models::PendingAction;
use super::state::App;

/// Run the TUI event loop
pub async fn run_tui(config: AppConfig) -> Result<Option<(Connection, PendingAction)>> {
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::prelude::*;
    use std::io::stdout;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(config)?;

    // Main loop
    loop {
        // Auto-clear old status messages
        app.maybe_clear_status();

        // Drain any completed ping results from background tasks
        app.drain_ping_results();

        // Drain any completed SFTP operation results
        app.drain_sftp_results();

        // Draw UI
        terminal.draw(|frame| {
            super::ui::draw(frame, &app);
        })?;

        // Handle events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key_event(key)?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    // Return selected connection and action if any
    Ok(app.selected_connection.zip(app.pending_action))
}
