use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::services::{NewConnection, SshService};
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
        .add_connection(NewConnection {
            name: new_name.clone(),
            host: original.host,
            user: Some(original.user),
            port: Some(original.port),
            kerberos: Some(original.use_kerberos),
            bastion: original.bastion,
            no_bastion: direct,
            bastion_user: original.bastion_user,
            key: original.key_path,
            tags: original.tags,
            forward_agent: original.forward_agent,
        })
        .await?;

    println!(
        "✅ Successfully duplicated '{}' to '{}'",
        original.name, new_name
    );

    Ok(())
}
