use crate::cli::utils::{confirm, resolve_connection};
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(
    target: Option<String>,
    tag: Option<String>,
    force: bool,
    config: AppConfig,
) -> Result<()> {
    let ssh_service = SshService::new(config)?;

    if let Some(tag_name) = tag {
        let connections = ssh_service.list_connections(Some(&tag_name), false).await?;
        if connections.is_empty() {
            println!("No connections found matching tag '{}'.", tag_name);
            return Ok(());
        }

        println!(
            "\n⚠️  WARNING: You are about to remove {} connection(s) matching tag '{}':",
            connections.len(),
            tag_name
        );
        for conn in &connections {
            println!(" - {} ({}@{})", conn.name, conn.user, conn.host);
        }

        if !force
            && !confirm(
                &format!("Remove all {} connection(s)?", connections.len()),
                false,
            )?
        {
            println!("❌ Removal cancelled.");
            return Ok(());
        }

        let mut removed = 0;
        for conn in &connections {
            if ssh_service.remove_connection(&conn.name).await? {
                removed += 1;
            }
        }
        println!(
            "✅ Removed {} connection(s) matching tag '{}'.",
            removed, tag_name
        );
        return Ok(());
    }

    let target_str = match target {
        Some(t) => t,
        None => {
            let connections = ssh_service.list_connections(None, false).await?;
            if connections.is_empty() {
                println!("No saved connections to remove.");
                return Ok(());
            }
            println!("No target specified. Available connections:\n");
            for (i, conn) in connections.iter().enumerate() {
                let tags = if conn.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", conn.tags.join(", "))
                };
                println!(
                    "  {:<3} {}@{}:{}{}",
                    i + 1,
                    conn.name,
                    conn.user,
                    conn.host,
                    tags
                );
            }
            println!("\nUse `bssh remove <name>` or `bssh remove --tag <tag>`.");
            return Ok(());
        }
    };

    info!("Removing connection: {}", target_str);
    let connection = resolve_connection(&ssh_service, &target_str, "remove", false).await?;
    remove_connection_with_confirmation(&ssh_service, &connection, force).await
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
            println!(
                "\n✅ Connection '{}' removed successfully!",
                connection.name
            );
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
