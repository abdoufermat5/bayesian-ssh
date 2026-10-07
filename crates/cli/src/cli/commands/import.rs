use crate::config::AppConfig;
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
    for host in parse_ssh_config(&content) {
        if let Err(e) = import_host(&ssh_service, &host, no_bastion).await {
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

/// One concrete `Host` alias from an OpenSSH config file.
#[derive(Debug, Default, Clone, PartialEq)]
struct ParsedHost {
    alias: String,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
}

/// Parse the subset of ssh_config(5) that maps onto a connection.
///
/// Follows OpenSSH lexing rules: keywords are case-insensitive, the value may
/// be separated by whitespace or `=`, values may be double-quoted, and the
/// first value seen for a keyword wins. A `Host` line with several patterns
/// yields one entry per concrete (non-wildcard, non-negated) alias. `Match`
/// blocks end the current `Host` block so their directives are not
/// misattributed to it.
fn parse_ssh_config(content: &str) -> Vec<ParsedHost> {
    let mut hosts: Vec<ParsedHost> = Vec::new();
    // Index range in `hosts` covered by the current Host block.
    let mut block: Option<std::ops::Range<usize>> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (keyword, value) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => {
                let rest = line[i..].trim_start();
                let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
                (&line[..i], rest)
            }
            None => (line, ""),
        };
        let keyword = keyword.to_ascii_lowercase();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);

        match keyword.as_str() {
            "host" => {
                let start = hosts.len();
                hosts.extend(
                    value
                        .split_whitespace()
                        .filter(|p| !p.starts_with('!') && !p.contains(['*', '?']))
                        .map(|p| ParsedHost {
                            alias: p.to_string(),
                            ..Default::default()
                        }),
                );
                block = Some(start..hosts.len());
            }
            "match" => block = None,
            _ => {
                let Some(range) = block.clone() else {
                    continue;
                };
                for host in &mut hosts[range] {
                    match keyword.as_str() {
                        "hostname" => {
                            host.hostname.get_or_insert_with(|| value.to_string());
                        }
                        "user" => {
                            host.user.get_or_insert_with(|| value.to_string());
                        }
                        "port" => {
                            if host.port.is_none() {
                                host.port = value.parse::<u16>().ok().filter(|&p| p != 0);
                            }
                        }
                        "identityfile" => {
                            host.identity_file.get_or_insert_with(|| value.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    hosts
}

async fn import_host(ssh_service: &SshService, host: &ParsedHost, no_bastion: bool) -> Result<()> {
    // Skip if host already exists
    if ssh_service.get_connection(&host.alias).await?.is_some() {
        return Ok(());
    }

    // Use HostName if available, otherwise fall back to Host
    let actual_host = host.hostname.clone().unwrap_or_else(|| host.alias.clone());

    ssh_service
        .add_connection(
            host.alias.clone(), // Use Host as the connection name
            actual_host,        // Use HostName as the actual host
            host.user.clone(),
            host.port,
            None,       // kerberos
            None,       // bastion
            no_bastion, // use the parameter passed from command line
            None,       // bastion_user
            host.identity_file.clone(),
            vec!["imported".to_string()],
        )
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitive_keywords_and_equals() {
        let cfg = "Host web\n  Hostname web.example.com\n  user=deploy\n  PORT = 2222\n  IdentityFile \"~/.ssh/my key\"\n";
        let hosts = parse_ssh_config(cfg);
        assert_eq!(
            hosts,
            vec![ParsedHost {
                alias: "web".into(),
                hostname: Some("web.example.com".into()),
                user: Some("deploy".into()),
                port: Some(2222),
                identity_file: Some("~/.ssh/my key".into()),
            }]
        );
    }

    #[test]
    fn match_block_does_not_leak_into_previous_host() {
        let cfg = "Host a\n  HostName a.example\nMatch host b\n  User root\n";
        let hosts = parse_ssh_config(cfg);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].user, None);
    }

    #[test]
    fn multi_pattern_host_skips_wildcards_and_first_value_wins() {
        let cfg = "Host *\n  User global\nHost db1 db2 *.corp !bad\n  User one\n  User two\n";
        let hosts = parse_ssh_config(cfg);
        let names: Vec<_> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["db1", "db2"]);
        assert!(hosts.iter().all(|h| h.user.as_deref() == Some("one")));
    }
}
