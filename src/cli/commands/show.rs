use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Showing details for connection: {}", target);

    let ssh_service = SshService::new(config)?;

    if let Some(connection) = ssh_service.get_connection(&target).await? {
        println!("🔗 Connection Details: {}\n", connection.name);
        println!("ID: {}", connection.id);
        println!("Host: {}:{}", connection.host, connection.port);
        println!("User: {}", connection.user);

        if let Some(bastion) = &connection.bastion {
            println!(
                "Bastion: {}@{}",
                connection
                    .bastion_user
                    .as_deref()
                    .unwrap_or(&connection.user),
                bastion
            );
        }

        println!(
            "Kerberos: {}",
            if connection.use_kerberos {
                "✅ Enabled"
            } else {
                "❌ Disabled"
            }
        );

        if let Some(key) = &connection.key_path {
            println!("SSH Key: {}", key);
        }

        println!(
            "Created: {}",
            connection.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );

        if let Some(last_used) = connection.last_used {
            println!("Last used: {}", last_used.format("%Y-%m-%d %H:%M:%S UTC"));
        }

        if !connection.tags.is_empty() {
            println!("Tags: {}", connection.tags.join(", "));
        }

        println!("\nSSH Command: {}", connection.to_ssh_command());
    } else {
        println!("❌ Connection '{}' not found.", target);
        println!("Use 'bayesian-ssh add' to create a new connection.");
    }

    Ok(())
}
