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
    if ssh_service.connection(&new_name).await?.is_some() {
        anyhow::bail!("A connection with the name '{}' already exists", new_name);
    }

    info!("Duplicating connection {} to {}", original.name, new_name);

    // A connection copied without a bastion must stay direct.
    let direct = original.bastion.is_none();

    ssh_service
        .add_connection(
            new_name.clone(),
            original.host,
            Some(original.user),
            Some(original.port),
            Some(original.use_kerberos),
            original.bastion,
            direct,
            original.bastion_user,
            original.key_path,
            original.tags,
            original.forward_agent,
        )
        .await?;

    println!(
        "✅ Successfully duplicated '{}' to '{}'",
        original.name, new_name
    );

    Ok(())
}
