use anyhow::Result;
use crate::config::AppConfig;
use crate::services::SshService;
use tracing::info;

pub async fn execute(
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    tags: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    info!("Adding new connection: {} -> {}", name, host);
    
    let ssh_service = SshService::new(config)?;
    
    ssh_service.add_connection(
        name.clone(),
        host,
        user,
        port,
        kerberos,
        bastion,
        no_bastion,
        bastion_user,
        key,
        tags,
    ).await?;
    
    println!("✅ Connection '{}' added successfully!", name);
    
    Ok(())
}
