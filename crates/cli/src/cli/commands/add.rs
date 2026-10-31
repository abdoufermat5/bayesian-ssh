use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

/// What `bssh add` was asked to save.
pub struct AddArgs {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub kerberos: Option<bool>,
    pub bastion: Option<String>,
    pub no_bastion: bool,
    pub bastion_user: Option<String>,
    pub key: Option<String>,
    pub tags: Vec<String>,
    pub forward_agent: bool,
}

pub async fn execute(args: AddArgs, config: AppConfig) -> Result<()> {
    let AddArgs {
        name,
        host,
        user,
        port,
        kerberos,
        bastion,
        no_bastion,
        bastion_user,
        key,
        tags,
        forward_agent,
    } = args;
    info!("Adding new connection: {} -> {}", name, host);

    let ssh_service = SshService::new(config)?;

    ssh_service
        .add_connection(
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
            forward_agent,
        )
        .await?;

    println!("✅ Connection '{}' added successfully!", name);

    Ok(())
}
