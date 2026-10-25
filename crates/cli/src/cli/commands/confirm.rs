use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

/// Accept the route changes a team sync made to a connection.
pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Confirming pending change for connection: {}", target);

    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "confirm", true, &config).await?;

    let Some(change) = &connection.pending_change else {
        println!("✅ '{}' has no pending change to confirm.", connection.name);
        return Ok(());
    };

    println!(
        "⚠️  '{}' was changed by a team sync ({}):",
        connection.name,
        change.detected_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    for line in crate::services::pending::describe(&connection) {
        println!("     {line}");
    }
    if connection.forward_agent {
        println!("   Agent forwarding stays on for this connection.");
    } else {
        println!("   Agent forwarding is off for this connection.");
    }

    ssh_service
        .database()
        .clear_pending_change(&connection.id)?;
    println!(
        "\n✅ Confirmed. The next connection to '{}' uses the new values.",
        connection.name
    );
    Ok(())
}
