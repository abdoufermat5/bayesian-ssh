use anyhow::Result;
use crate::config::AppConfig;
use crate::services::SshService;
use tracing::info;

pub async fn execute(
    target: String,
    name: Option<String>,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    bastion_user: Option<String>,
    key: Option<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    info!("Editing connection: {}", target);
    
    let ssh_service = SshService::new(config)?;
    
    if let Some(mut connection) = ssh_service.get_connection(&target).await? {
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
            println!("  Bastion: {}@{}", 
                connection.bastion_user.as_deref().unwrap_or(&connection.user), 
                bastion
            );
        }
        println!("  Kerberos: {}", if connection.use_kerberos { "✅ Enabled" } else { "❌ Disabled" });
        if !connection.tags.is_empty() {
            println!("  Tags: {}", connection.tags.join(", "));
        }
        
    } else {
        println!("❌ Connection '{}' not found.", target);
        println!("Use 'bayesian-ssh add' to create a new connection.");
    }
    
    Ok(())
}
