use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    target: String,
    name: Option<String>,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    info!("Editing connection: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try exact match
    if let Some(connection) = ssh_service.get_connection(&target).await? {
        return update_connection(
            ssh_service,
            connection,
            name,
            host,
            user,
            port,
            kerberos,
            bastion,
            no_bastion,
            bastion_user,
            key,
            add_tags,
            remove_tags,
        )
        .await;
    }

    // No exact match, try fuzzy search
    info!(
        "No exact match found for '{}', attempting fuzzy search",
        target
    );

    // Use shared fuzzy selection (without auto-select for edit operations)
    if let Some(connection) = fuzzy_select_connection(
        &ssh_service,
        &target,
        "edit",
        false, // Don't auto-select - require confirmation for edits
    )
    .await?
    {
        return update_connection(
            ssh_service,
            connection,
            name,
            host,
            user,
            port,
            kerberos,
            bastion,
            no_bastion,
            bastion_user,
            key,
            add_tags,
            remove_tags,
        )
        .await;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_connection(
    ssh_service: SshService,
    mut connection: crate::models::Connection,
    name: Option<String>,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
) -> Result<()> {
    // Update fields if provided
    if let Some(name) = name {
        connection.name = name;
    }
    if let Some(host) = host {
        connection.host = host;
    }
    if let Some(user) = user {
        connection.user = user;
    }
    if let Some(port) = port {
        connection.port = port;
    }
    if let Some(kerberos) = kerberos {
        connection.use_kerberos = kerberos;
    }
    if let Some(bastion) = bastion {
        connection.bastion = Some(bastion);
    }
    if no_bastion {
        connection.bastion = None;
        connection.bastion_user = None;
    }
    if let Some(bastion_user) = bastion_user {
        connection.bastion_user = Some(bastion_user);
    }
    if let Some(key) = key {
        connection.key_path = Some(key);
    }

    // Handle tags
    for tag in add_tags {
        connection.add_tag(tag);
    }
    for tag in remove_tags {
        connection.remove_tag(&tag);
    }

    // Update in database
    ssh_service.update_connection(connection.clone()).await?;

    println!("✅ Connection '{}' updated successfully!", connection.name);
    println!("\nUpdated connection details:");
    println!("  Name: {}", connection.name);
    println!("  Host: {}:{}", connection.host, connection.port);
    println!("  User: {}", connection.user);
    if let Some(bastion) = &connection.bastion {
        println!(
            "  Bastion: {}@{}",
            connection
                .bastion_user
                .as_deref()
                .unwrap_or(&connection.user),
            bastion
        );
    }
    println!(
        "  Kerberos: {}",
        if connection.use_kerberos {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    if !connection.tags.is_empty() {
        println!("  Tags: {}", connection.tags.join(", "));
    }

    Ok(())
}
