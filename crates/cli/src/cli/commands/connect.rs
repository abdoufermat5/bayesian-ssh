use crate::cli::utils::resolve_connection;
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

    let ssh_service = SshService::new(config.clone())?;

    let connection = match resolve_connection(&ssh_service, &target, "connect to", true).await {
        Ok(c) => c,
        Err(_) => {
            info!(
                "Connection not resolved, attempting direct connection to {}",
                target
            );
            crate::models::Connection::new(
                target.clone(),
                target.clone(),
                user.clone().unwrap_or_else(|| config.default_user.clone()),
                port.unwrap_or(config.default_port),
                bastion.clone(),
                bastion_user.clone(),
                kerberos.unwrap_or(config.use_kerberos_by_default),
                key.clone(),
            )
        }
    };

    ssh_service
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
        .await
}
