use crate::config::AppConfig;
use crate::services::inventory::ssh_config::{
    parse_ssh_config, resolve_bastion, BastionRoute, ParsedHost,
};
use crate::services::SshService;
use anyhow::{anyhow, bail, Result};
use tracing::info;

pub async fn execute(
    file: Option<String>,
    no_bastion: bool,
    passphrase: Option<String>,
    config: AppConfig,
) -> Result<()> {
    let target_path = match file {
        Some(file) => std::path::PathBuf::from(file),
        None => match config.ssh_config_path.clone() {
            Some(path) => path,
            None => dirs::home_dir()
                .ok_or_else(|| {
                    anyhow!(
                        "Unable to determine home directory. Please specify the SSH config path with --file"
                    )
                })?
                .join(".ssh/config"),
        },
    };

    info!("Importing connections from file: {:?}", target_path);

    if !target_path.exists() {
        bail!("Import file not found: {}", target_path.display());
    }

    let raw_bytes = std::fs::read(&target_path)?;

    // Check if encrypted or if passphrase is provided
    let content_bytes = if raw_bytes.starts_with(b"BSSH") || passphrase.is_some() {
        let pass = passphrase.ok_or_else(|| {
            anyhow!("File is encrypted with passphrase. Please provide --passphrase <secret>")
        })?;
        println!("🔓 Decrypting import file with passphrase...");
        crate::services::crypto::decrypt_data(&raw_bytes, &pass)?
    } else {
        raw_bytes
    };

    let content = String::from_utf8_lossy(&content_bytes);
    let ssh_service = SshService::new(config)?;

    // Try parsing as JSON array of Connection models first
    if let Ok(connections) = serde_json::from_str::<Vec<crate::models::Connection>>(&content) {
        let (mut imported, mut skipped) = (0, 0);
        for conn in connections {
            // Same policy as the ssh-config path: never clobber or abort on
            // an existing name, so re-importing a backup is idempotent.
            if ssh_service.get_connection(&conn.name).await?.is_some() {
                skipped += 1;
                continue;
            }
            let name = conn.name.clone();
            // A connection exported without a bastion must stay direct; do
            // not let the configured default bastion leak into it.
            let direct = no_bastion || conn.bastion.is_none();
            if let Err(e) = ssh_service
                .add_connection(
                    conn.name,
                    conn.host,
                    Some(conn.user),
                    Some(conn.port),
                    Some(conn.use_kerberos),
                    conn.bastion,
                    direct,
                    conn.bastion_user,
                    conn.key_path,
                    conn.tags,
                )
                .await
            {
                eprintln!("Warning: Failed to import connection '{}': {}", name, e);
            } else {
                imported += 1;
            }
        }
        println!(
            "✅ Imported {} connection(s) from JSON backup ({} already existed).",
            imported, skipped
        );
        return Ok(());
    }

    let mut imported_count = 0;
    let hosts = parse_ssh_config(&content);
    for host in &hosts {
        if let Err(e) = import_host(&ssh_service, &hosts, host, no_bastion).await {
            eprintln!("Warning: Failed to import host '{}': {}", host.alias, e);
        } else {
            imported_count += 1;
        }
    }

    println!(
        "✅ Successfully imported {} connection(s) from SSH config",
        imported_count
    );

    Ok(())
}

async fn import_host(
    ssh_service: &SshService,
    all: &[ParsedHost],
    host: &ParsedHost,
    no_bastion: bool,
) -> Result<()> {
    // Skip if host already exists
    if ssh_service.get_connection(&host.alias).await?.is_some() {
        return Ok(());
    }

    // Use HostName if available, otherwise fall back to Host
    let actual_host = host.hostname.clone().unwrap_or_else(|| host.alias.clone());

    // ProxyJump / `ssh -W %h:%p` ProxyCommand become the connection's bastion.
    let mut direct = no_bastion;
    let (mut bastion, mut bastion_user) = (None, None);
    if !no_bastion {
        let resolved = resolve_bastion(all, host);
        for message in &resolved.warnings {
            eprintln!("Warning: host '{}': {}", host.alias, message);
        }
        match resolved.route {
            BastionRoute::Unset => {}
            // `ProxyJump none` must not pick up the configured default bastion.
            BastionRoute::Direct => direct = true,
            BastionRoute::Hop { bastion: b, user } => {
                bastion = Some(b);
                bastion_user = user;
            }
        }
    }

    ssh_service
        .add_connection(
            host.alias.clone(), // Use Host as the connection name
            actual_host,        // Use HostName as the actual host
            host.user.clone(),
            host.port,
            None, // kerberos
            bastion,
            direct, // --no-bastion or `ProxyJump none`
            bastion_user,
            host.identity_file.clone(),
            vec!["imported".to_string()],
        )
        .await?;

    Ok(())
}
