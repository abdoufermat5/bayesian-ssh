use crate::cli::utils::{fuzzy_select_connection, show_connection_details};
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Showing details for connection: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try exact match
    if let Some(connection) = ssh_service.get_connection(&target).await? {
        return show_connection_details(&connection);
    }

    // No exact match, try fuzzy search
    info!(
        "No exact match found for '{}', attempting fuzzy search",
        target
    );

    // Use shared fuzzy selection with auto-select for single matches (show is non-destructive)
    if let Some(connection) = fuzzy_select_connection(
        &ssh_service,
        &target,
        "show",
        true, // auto_select_single - show is non-destructive, skip confirmation
    )
    .await?
    {
        return show_connection_details(&connection);
    }

    Ok(())
}
