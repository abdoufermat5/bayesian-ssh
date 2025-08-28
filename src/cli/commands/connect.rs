use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use std::io::{self, Write};
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

    let ssh_service = SshService::new(config)?;

    // First try to connect with exact match (existing behavior)
    match ssh_service
        .connect(
            &target,
            user.clone(),
            port,
            kerberos,
            bastion.clone(),
            no_bastion,
            bastion_user.clone(),
            key.clone(),
        )
        .await
    {
        Ok(_) => return Ok(()), // Exact match found and connected
        Err(_) => {
            // No exact match, try fuzzy search
            info!("No exact match found for '{}', attempting fuzzy search", target);
        }
    }

    // Perform fuzzy search
    let matches = ssh_service.fuzzy_search(&target, 10).await?;

    match matches.len() {
        0 => {
            println!("❌ No connections found matching '{}'", target);

            // Show recent connections as fallback
            let recent = ssh_service.get_recent_connections(5).await?;
            if !recent.is_empty() {
                println!("\n📅 Recent connections:");
                for (i, conn) in recent.iter().enumerate() {
                    let last_used = conn.last_used
                        .map(|dt| format!(" (last used: {})", format_duration(dt)))
                        .unwrap_or_else(|| "".to_string());
                    println!("  {}. {}{}", i + 1, conn.name, last_used);
                }

                if let Some(selection) = interactive_selection(&recent, "Select recent connection")? {
                    return ssh_service.connect_to_connection(&selection, user, port, kerberos, bastion, no_bastion, bastion_user, key).await;
                }
            } else {
                println!("No recent connections found.");
            }
            return Ok(());
        }
        1 => {
            // Single match - ask for confirmation
            let conn = &matches[0];
            println!("🔍 Found one similar connection:");
            print_connection_info(conn, 1);

            print!("Connect to this server? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();

            if matches!(input.as_str(), "y" | "yes") {
                return ssh_service.connect_to_connection(conn, user, port, kerberos, bastion, no_bastion, bastion_user, key).await;
            } else {
                println!("Connection cancelled.");
                return Ok(());
            }
        }
        _ => {
            // Multiple matches - interactive selection
            println!("🔍 Found {} similar connections for '{}':", matches.len(), target);
            println!();

            for (i, conn) in matches.iter().enumerate() {
                print_connection_info(conn, i + 1);
            }

            if let Some(selection) = interactive_selection(&matches, "Select connection")? {
                return ssh_service.connect_to_connection(&selection, user, port, kerberos, bastion, no_bastion, bastion_user, key).await;
            }
        }
    }

    Ok(())
}

fn print_connection_info(connection: &crate::models::Connection, index: usize) {
    let tags_str = if connection.tags.is_empty() {
        "".to_string()
    } else {
        format!(" [{}]", connection.tags.join(", "))
    };

    let last_used = connection.last_used
        .map(|dt| format!(" (last used: {})", format_duration(dt)))
        .unwrap_or_else(|| "".to_string());

    println!("  {}. {} ({})", index, connection.name, connection.host);
    println!("     Tags: {}{}", if tags_str.is_empty() { "none" } else { &tags_str[1..tags_str.len()-1] }, last_used);
    println!();
}

fn format_duration(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(dt);

    if duration.num_days() > 0 {
        if duration.num_days() == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", duration.num_days())
        }
    } else if duration.num_hours() > 0 {
        if duration.num_hours() == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", duration.num_hours())
        }
    } else if duration.num_minutes() > 0 {
        if duration.num_minutes() == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", duration.num_minutes())
        }
    } else {
        "just now".to_string()
    }
}

fn interactive_selection(connections: &[crate::models::Connection], prompt: &str) -> Result<Option<crate::models::Connection>> {
    loop {
        print!("{} [1-{}, 's' to search again, 'q' to quit]: ", prompt, connections.len());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "q" | "quit" => {
                println!("Connection cancelled.");
                return Ok(None);
            }
            "s" | "search" => {
                print!("Enter new search term: ");
                io::stdout().flush()?;

                let mut new_search = String::new();
                io::stdin().read_line(&mut new_search)?;
                let new_search = new_search.trim();

                if new_search.is_empty() {
                    continue;
                }

                // This would require passing the ssh_service reference
                // For now, we'll just continue the loop
                println!("New search functionality would be implemented here.");
                continue;
            }
            _ => {
                if let Ok(index) = input.parse::<usize>() {
                    if index >= 1 && index <= connections.len() {
                        return Ok(Some(connections[index - 1].clone()));
                    } else {
                        println!("Invalid selection. Please enter a number between 1 and {}.", connections.len());
                    }
                } else {
                    println!("Invalid input. Please enter a number, 's' to search again, or 'q' to quit.");
                }
            }
        }
    }
}
