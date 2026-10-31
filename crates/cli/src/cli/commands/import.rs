use crate::config::AppConfig;
use crate::services::inventory::ssh_config::{
    parse_ssh_config, resolve_bastion, BastionRoute, ParsedHost,
};
use crate::services::{NewConnection, SshService};
use anyhow::{anyhow, bail, Result};
use tracing::info;

/// How an import treats bastion routes declared in the SSH config.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BastionImport {
    /// Resolve `ProxyJump` / `ProxyCommand` routes into a bastion.
    Resolve,
    /// `--no-bastion`: import every host as a direct connection.
    ForceDirect,
}

impl From<bool> for BastionImport {
    fn from(no_bastion: bool) -> Self {
        if no_bastion {
            Self::ForceDirect
        } else {
            Self::Resolve
        }
    }
}

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
            if ssh_service.connection(&conn.name).await?.is_some() {
                skipped += 1;
                continue;
            }
            let name = conn.name.clone();
            // A connection exported without a bastion must stay direct; do
            // not let the configured default bastion leak into it.
            let direct = no_bastion || conn.bastion.is_none();
            if let Err(e) = ssh_service
                .add_connection(NewConnection {
                    name: conn.name,
                    host: conn.host,
                    user: Some(conn.user),
                    port: Some(conn.port),
                    kerberos: Some(conn.use_kerberos),
                    bastion: conn.bastion,
                    no_bastion: direct,
                    bastion_user: conn.bastion_user,
                    key: conn.key_path,
                    tags: conn.tags,
                    forward_agent: conn.forward_agent,
                })
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
    let import = SshConfigImport {
        service: &ssh_service,
        hosts: &hosts,
        policy: BastionImport::from(no_bastion),
    };
    for host in &hosts {
        if let Err(e) = import.host(host).await {
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

/// One SSH-config import run: the parsed hosts, the bastion policy and the
/// service new connections are written through.
struct SshConfigImport<'a> {
    service: &'a SshService,
    hosts: &'a [ParsedHost],
    policy: BastionImport,
}

impl SshConfigImport<'_> {
    async fn host(&self, host: &ParsedHost) -> Result<()> {
        // Skip if host already exists
        if self.service.connection(&host.alias).await?.is_some() {
            return Ok(());
        }

        // Use HostName if available, otherwise fall back to Host
        let actual_host = host.hostname.clone().unwrap_or_else(|| host.alias.clone());

        // ProxyJump / `ssh -W %h:%p` ProxyCommand become the connection's bastion.
        let mut direct = self.policy == BastionImport::ForceDirect;
        let (mut bastion, mut bastion_user) = (None, None);
        if self.policy == BastionImport::Resolve {
            let resolved = resolve_bastion(self.hosts, host);
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

        self.service
            .add_connection(NewConnection {
                // Host is the connection name, HostName the actual host.
                name: host.alias.clone(),
                host: actual_host,
                user: host.user.clone(),
                port: host.port,
                kerberos: None,
                bastion,
                // --no-bastion or `ProxyJump none`
                no_bastion: direct,
                bastion_user,
                key: host.identity_file.clone(),
                tags: vec!["imported".to_string()],
                // Agent forwarding is a local, per-connection decision.
                forward_agent: false,
            })
            .await?;

        Ok(())
    }
}
