use crate::cli::utils::{confirm, fuzzy_select_connection};
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(target: String, force: bool, config: AppConfig) -> Result<()> {
    info!("Removing connection: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try exact match
    if let Some(connection) = ssh_service.get_connection(&target).await? {
        return remove_connection_with_confirmation(&ssh_service, &connection, force).await;
    }

    // No exact match, try fuzzy search
    info!(
        "No exact match found for '{}', attempting fuzzy search",
        target
    );

    // Use shared fuzzy selection (without auto-select for remove operations)
    if let Some(connection) = fuzzy_select_connection(
        &ssh_service,
        &target,
        "remove",
        false, // Don't auto-select - require confirmation for deletions
    )
    .await?
    {
        return remove_connection_with_confirmation(&ssh_service, &connection, force).await;
    }

    Ok(())
}

async fn remove_connection_with_confirmation(
    ssh_service: &SshService,
    connection: &crate::models::Connection,
    force: bool,
) -> Result<()> {
    println!("\n⚠️  WARNING: You are about to remove the following connection:");
    println!("   Name: {}", connection.name);
    println!("   Host: {}:{}", connection.host, connection.port);
    println!("   User: {}", connection.user);

    if !connection.tags.is_empty() {
        println!("   Tags: {}", connection.tags.join(", "));
    }

    // If force flag is set, skip confirmation
    if force {
        if ssh_service.remove_connection(&connection.name).await? {
            println!("\n✅ Connection '{}' removed successfully!", connection.name);
        } else {
            println!("\n❌ Failed to remove connection '{}'", connection.name);
        }
        return Ok(());
    }

    // Ask for confirmation
    println!();
    if confirm(&format!("Remove connection '{}'?", connection.name), false)? {
        if ssh_service.remove_connection(&connection.name).await? {
            println!("✅ Connection '{}' removed successfully!", connection.name);
        } else {
            println!("❌ Failed to remove connection '{}'", connection.name);
        }
    } else {
        println!("❌ Removal cancelled.");
    }

    Ok(())
}
