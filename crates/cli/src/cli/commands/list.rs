use crate::cli::utils::{format_duration, truncate_display};
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

pub async fn execute(
    tag: Option<String>,
    recent: bool,
    detailed: bool,
    config: AppConfig,
) -> Result<()> {
    info!(
        "Listing connections (tag: {:?}, recent: {}, detailed: {})",
        tag, recent, detailed
    );

    let ssh_service = SshService::new(config.clone())?;
    let connections = ssh_service.list_connections(tag.as_deref(), recent).await?;
    let source_names: std::collections::HashMap<_, _> = crate::database::Database::new(&config)?
        .list_sources()?
        .into_iter()
        .map(|summary| (summary.source.id, summary.source.name))
        .collect();

    if connections.is_empty() {
        println!("📭 No connections found.");
        if tag.is_some() {
            println!("   Try without --tag filter or use 'bssh add' to create connections.");
        }
        return Ok(());
    }

    // Header
    let filter_info = match (&tag, recent) {
        (Some(t), true) => format!(" (tag: {}, recent only)", t),
        (Some(t), false) => format!(" (tag: {})", t),
        (None, true) => " (recent only)".to_string(),
        (None, false) => String::new(),
    };

    println!("\n🔗 Connections{}", filter_info);
    println!("{}", "═".repeat(70));

    if detailed {
        for conn in &connections {
            // Connection header
            let status_icons = format!(
                "{}{}",
                if conn.use_kerberos { "🔐" } else { "" },
                if conn.bastion.is_some() { "🔗" } else { "" }
            );

            println!("\n┌─ {} {}", conn.name, status_icons);
            println!("│  {}@{}:{}", conn.user, conn.host, conn.port);

            if let Some(bastion) = &conn.bastion {
                println!(
                    "│  via {}@{}",
                    conn.bastion_user.as_deref().unwrap_or(&conn.user),
                    bastion
                );
            }

            if let Some(key) = &conn.key_path {
                println!("│  key: {}", key);
            }

            if !conn.tags.is_empty() {
                println!("│  tags: {}", conn.tags.join(", "));
            }

            if let Some(source) = conn.source_id.and_then(|id| source_names.get(&id)) {
                println!("│  source: {}", source);
            }

            if let Some(stale_since) = conn.stale_since {
                println!("│  stale since: {}", format_duration(stale_since));
            }

            if let Some(last_used) = conn.last_used {
                println!("│  last: {}", format_duration(last_used));
            }

            println!("└─ id: {}", conn.id);
        }
    } else {
        // Table header
        println!(
            "{:<3} {:<20} {:<25} {:<8} INFO",
            "#", "NAME", "HOST", "PORT"
        );
        println!("{}", "─".repeat(70));

        for (i, conn) in connections.iter().enumerate() {
            let icons = format!(
                "{}{}",
                if conn.use_kerberos { "🔐" } else { "  " },
                if conn.bastion.is_some() { "🔗" } else { "  " }
            );

            let last_used = conn.last_used.map(format_duration).unwrap_or_default();

            let tags = if !conn.tags.is_empty() {
                format!("[{}]", conn.tags.join(","))
            } else {
                String::new()
            };

            let info = format!("{} {} {}", icons, last_used, tags)
                .trim()
                .to_string();
            let info = match conn.source_id.and_then(|id| source_names.get(&id)) {
                Some(source) => format!("[{}] {}", source, info),
                None => info,
            };
            let name = if conn.stale_since.is_some() {
                format!("{} (stale)", truncate_display(&conn.name, 11))
            } else {
                conn.name.clone()
            };

            println!(
                "{:<3} {:<20} {:<25} {:<8} {}",
                i + 1,
                truncate_display(&name, 19),
                format!(
                    "{}@{}",
                    truncate_display(&conn.user, 8),
                    truncate_display(&conn.host, 15)
                ),
                conn.port,
                info
            );
        }
    }

    println!("{}", "─".repeat(70));
    println!("Total: {} connection(s)", connections.len());
    println!();

    Ok(())
}
