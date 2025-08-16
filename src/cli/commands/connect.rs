use anyhow::Result;
use crate::config::AppConfig;
use crate::services::SshService;
use tracing::info;

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
    
    ssh_service.connect(
        &target,
        user,
        port,
        kerberos,
        bastion,
        no_bastion,
        bastion_user,
        key,
    ).await?;
    
    Ok(())
}
