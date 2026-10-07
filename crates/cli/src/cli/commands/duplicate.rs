use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(source: String, new_name: String, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;

    // Exact name/alias lookup first, then interactive fuzzy search.
    let original = resolve_connection(&ssh_service, &source, "duplicate", true, &config).await?;

    // Check if new name already exists
    if ssh_service.get_connection(&new_name).await?.is_some() {
        anyhow::bail!("A connection with the name '{}' already exists", new_name);
    }

    info!("Duplicating connection {} to {}", original.name, new_name);

    ssh_service
        .add_connection(
            new_name.clone(),
            original.host.clone(),
            Some(original.user.clone()),
            Some(original.port),
            Some(original.use_kerberos),
            original.bastion.clone(),
            original.bastion.is_none(),
            original.bastion_user.clone(),
            original.key_path.clone(),
            original.tags.clone(),
        )
        .await?;

    println!(
        "✅ Successfully duplicated '{}' to '{}'",
        original.name, new_name
    );

    Ok(())
}
