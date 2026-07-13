use crate::cli::utils::{resolve_connection, show_connection_details};
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Showing details for connection: {}", target);

    let ssh_service = SshService::new(config)?;
    let connection = resolve_connection(&ssh_service, &target, "show", true).await?;
    show_connection_details(&connection)
}
