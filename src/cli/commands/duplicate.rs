use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(source: String, new_name: String, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config)?;

    // Try to find the exact connection by name or alias first
    let mut target_conn = ssh_service
        .get_connection(&source)
        .await
        .unwrap_or_default();

    // Fall back to fuzzy search if not found
    if target_conn.is_none() {
        println!("🔍 Connection '{}' not found, searching...", source);
        target_conn = fuzzy_select_connection(&ssh_service, &source, "duplicate", true).await?;
    }

    let original = match target_conn {
        Some(c) => c,
        None => {
            println!("❌ No connection selected for duplication.");
            return Ok(());
        }
    };

    // Check if new name already exists
    if ssh_service
        .get_connection(&new_name)
        .await
        .unwrap_or_default()
        .is_some()
    {
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
