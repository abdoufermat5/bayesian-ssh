use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    target: String,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    config: AppConfig,
) -> Result<()> {
    info!("Connecting to target: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try to connect with exact match (existing behavior)
    match ssh_service
        .connect(
            &target,
            user.clone(),
            port,
            kerberos,
            bastion.clone(),
            no_bastion,
            bastion_user.clone(),
            key.clone(),
        )
        .await
    {
        Ok(_) => return Ok(()), // Exact match found and connected
        Err(_) => {
            // No exact match, try fuzzy search
            info!(
                "No exact match found for '{}', attempting fuzzy search",
                target
            );
        }
    }

    // Use the shared fuzzy selection with auto-connect for single matches
    if let Some(connection) = fuzzy_select_connection(
        &ssh_service,
        &target,
        "connect to",
        true, // auto_select_single - skip confirmation for single match
    )
    .await?
    {
        return ssh_service
            .connect_to_connection(
                &connection,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
            )
            .await;
    }

    Ok(())
}
