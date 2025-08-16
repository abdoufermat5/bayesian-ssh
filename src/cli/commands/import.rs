use anyhow::Result;
use crate::config::AppConfig;
use crate::services::SshService;
use tracing::info;
use std::path::Path;

pub async fn execute(
    file: Option<String>,
    no_bastion: bool,
    config: AppConfig,
) -> Result<()> {
    let ssh_config_path = if let Some(file) = file {
        std::path::PathBuf::from(file)
    } else {
        config.ssh_config_path.clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".ssh/config"))
    };
    
    info!("Importing connections from SSH config: {:?}", ssh_config_path);
    
    if !ssh_config_path.exists() {
        println!("❌ SSH config file not found: {:?}", ssh_config_path);
        return Ok(());
    }
    
    let ssh_service = SshService::new(config)?;
    let content = std::fs::read_to_string(&ssh_config_path)?;
    
    let mut imported_count = 0;
    let mut current_host: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port: Option<u16> = None;
    let mut current_identity_file: Option<String> = None;
    
    for line in content.lines() {
        let line = line.trim();
        
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        // Parse SSH config directives
        if line.starts_with("Host ") {
            // Save previous host if exists
            if let Some(host) = current_host.take() {
                            if let Err(e) = self::import_host(
                &ssh_service,
                &host,
                current_user.take(),
                current_port.take(),
                current_identity_file.take(),
                no_bastion,
            ).await {
                    eprintln!("Warning: Failed to import host '{}': {}", host, e);
                } else {
                    imported_count += 1;
                }
            }
            
            // Start new host
            current_host = Some(line[5..].trim().to_string());
            current_user = None;
            current_port = None;
            current_identity_file = None;
            
        } else if line.starts_with("User ") {
            current_user = Some(line[5..].trim().to_string());
        } else if line.starts_with("Port ") {
            if let Ok(port) = line[5..].trim().parse::<u16>() {
                current_port = Some(port);
            }
        } else if line.starts_with("IdentityFile ") {
            current_identity_file = Some(line[13..].trim().to_string());
        }
    }
    
    // Don't forget the last host
    if let Some(host) = current_host {
        if let Err(e) = self::import_host(
            &ssh_service,
            &host,
            current_user,
            current_port,
            current_identity_file,
            no_bastion,
        ).await {
            eprintln!("Warning: Failed to import host '{}': {}", host, e);
        } else {
            imported_count += 1;
        }
    }
    
    println!("✅ Successfully imported {} connection(s) from SSH config", imported_count);
    
    Ok(())
}

async fn import_host(
    ssh_service: &SshService,
    host: &str,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    no_bastion: bool,
) -> Result<()> {
    // Skip wildcard hosts
    if host.contains('*') || host.contains('?') {
        return Ok(());
    }
    
    // Skip if host already exists
    if ssh_service.get_connection(host).await?.is_some() {
        return Ok(());
    }
    
    ssh_service.add_connection(
        host.to_string(),
        host.to_string(),
        user,
        port,
        None, // kerberos
        None, // bastion
        no_bastion, // use the parameter passed from command line
        None, // bastion_user
        identity_file,
        vec!["imported".to_string()],
    ).await?;
    
    Ok(())
}
