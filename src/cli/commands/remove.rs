use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Removing connection: {}", target);

    let ssh_service = SshService::new(config)?;

    if ssh_service.remove_connection(&target).await? {
        println!("✅ Connection '{}' removed successfully!", target);
    } else {
        println!("❌ Connection '{}' not found.", target);
    }

    Ok(())
}
