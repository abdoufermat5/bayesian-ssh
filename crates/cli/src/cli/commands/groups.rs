use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use std::collections::HashMap;

pub async fn execute(group_name: Option<String>, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config)?;

    if let Some(tag) = group_name {
        // Exact, normalized tag match in SQL (same semantics as `list -t`).
        let filtered = ssh_service.list_connections(Some(&tag), false).await?;

        if filtered.is_empty() {
            println!("No connections found in group '{}'", tag);
        } else {
            println!("Connections in group '{}':", tag);
            for conn in filtered {
                println!("  - {} ({}@{})", conn.name, conn.user, conn.host);
            }
        }
    } else {
        // List all groups and their connection counts
        let connections = ssh_service.list_connections(None, false).await?;
        let mut group_counts: HashMap<String, usize> = HashMap::new();

        for conn in connections {
            for tag in conn.tags {
                *group_counts.entry(tag).or_insert(0) += 1;
            }
        }

        if group_counts.is_empty() {
            println!("No groups (tags) found.");
        } else {
            println!("Available groups:");
            let mut groups: Vec<_> = group_counts.into_iter().collect();
            groups.sort_by(|a, b| a.0.cmp(&b.0)); // Sort alphabetically

            for (tag, count) in groups {
                let suffix = if count == 1 {
                    "connection"
                } else {
                    "connections"
                };
                println!("  - {} ({} {})", tag, count, suffix);
            }
            println!("\nUse `bssh groups <name>` to see connections in a group.");
        }
    }

    Ok(())
}
