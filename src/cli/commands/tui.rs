//! TUI command implementation - launch interactive terminal UI

use crate::config::AppConfig;
use crate::services::SshService;
use crate::tui::app::{run_tui, PendingAction};
use crate::cli::utils::show_connection_details;
use anyhow::Result;

/// Execute the TUI command
pub async fn execute(config: AppConfig) -> Result<()> {
    // Run the TUI and get the result
    let result = run_tui(config.clone()).await?;

    // Handle any pending action after TUI exits
    if let Some((connection, action)) = result {
        match action {
            PendingAction::Connect => {
                println!("\n🔗 Connecting to {}...\n", connection.name);
                let ssh_service = SshService::new(config)?;
                ssh_service.connect_to_connection(&connection, None, None, None, None, false, None, None).await?;
            }
            PendingAction::ShowDetails => {
                println!();
                show_connection_details(&connection)?;
            }
        }
    }

    Ok(())
}
