use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(
    tag: Option<String>,
    recent: bool,
    detailed: bool,
    config: AppConfig,
) -> Result<()> {
    info!(
        "Listing connections (tag: {:?}, recent: {}, detailed: {})",
        tag, recent, detailed
    );

    let ssh_service = SshService::new(config)?;
    let connections = ssh_service.list_connections(tag.as_deref(), recent).await?;

    if connections.is_empty() {
        println!("📭 No connections found.");
        return Ok(());
    }

    println!("🔗 Found {} connection(s):\n", connections.len());

    for (i, conn) in connections.iter().enumerate() {
        if detailed {
            println!("{}. {} (ID: {})", i + 1, conn.name, conn.id);
            println!("   Host: {}:{}", conn.host, conn.port);
            println!("   User: {}", conn.user);
            if let Some(bastion) = &conn.bastion {
                println!(
                    "   Bastion: {}@{}",
                    conn.bastion_user.as_deref().unwrap_or(&conn.user),
                    bastion
                );
            }
            println!(
                "   Kerberos: {}",
                if conn.use_kerberos { "✅" } else { "❌" }
            );
            if let Some(key) = &conn.key_path {
                println!("   SSH Key: {}", key);
            }
            if let Some(last_used) = conn.last_used {
                println!("   Last used: {}", last_used.format("%Y-%m-%d %H:%M:%S"));
            }
            if !conn.tags.is_empty() {
                println!("   Tags: {}", conn.tags.join(", "));
            }
            println!();
        } else {
            let bastion_info = if let Some(bastion) = &conn.bastion {
                format!(
                    " → {}@{}",
                    conn.bastion_user.as_deref().unwrap_or(&conn.user),
                    bastion
                )
            } else {
                String::new()
            };

            let kerberos_icon = if conn.use_kerberos { "🔐" } else { "" };
            let last_used_info = if let Some(last_used) = conn.last_used {
                format!(" (last: {})", last_used.format("%m-%d %H:%M"))
            } else {
                String::new()
            };

            println!(
                "{}. {}{} {}{}{}",
                i + 1,
                conn.name,
                bastion_info,
                kerberos_icon,
                last_used_info,
                if !conn.tags.is_empty() {
                    format!(" [{}]", conn.tags.join(", "))
                } else {
                    String::new()
                }
            );
        }
    }

    Ok(())
}
