use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use std::io::{self, Write};
use tracing::info;

pub async fn execute(target: String, config: AppConfig) -> Result<()> {
    info!("Removing connection: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try exact match
    if ssh_service.remove_connection(&target).await? {
        println!("✅ Connection '{}' removed successfully!", target);
        return Ok(());
    }

    // No exact match, try fuzzy search
    info!(
        "No exact match found for '{}', attempting fuzzy search",
        target
    );

    let matches = ssh_service.fuzzy_search(&target, 10).await?;

    match matches.len() {
        0 => {
            println!("❌ No connections found matching '{}'", target);

            // Show recent connections as fallback
            let recent = ssh_service.get_recent_connections(5).await?;
            if !recent.is_empty() {
                println!("\nRecent connections:");
                for (i, conn) in recent.iter().enumerate() {
                    let last_used = conn
                        .last_used
                        .map(|dt| format!(" (last: {})", format_duration(dt)))
                        .unwrap_or_default();
                    println!("  {}. {}{}", i + 1, conn.name, last_used);
                }

                if let Some(selection) =
                    interactive_selection(&recent, "Select connection to remove")?
                {
                    return remove_connection_with_confirmation(&ssh_service, &selection).await;
                }
            } else {
                println!("No recent connections found.");
            }
        }
        1 => {
            // Single match - ask for confirmation
            let conn = &matches[0];
            println!("Found one similar connection:");
            print_connection_info(conn, 1);

            print!("Remove this connection? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();

            if matches!(input.as_str(), "y" | "yes") {
                return remove_connection_with_confirmation(&ssh_service, conn).await;
            } else {
                println!("Removal cancelled.");
            }
        }
        _ => {
            // Multiple matches - interactive selection
            println!(
                "Found {} similar connections for '{}':",
                matches.len(),
                target
            );
            println!();

            for (i, conn) in matches.iter().enumerate() {
                print_connection_info(conn, i + 1);
            }

            if let Some(selection) = interactive_selection(&matches, "Select connection to remove")?
            {
                return remove_connection_with_confirmation(&ssh_service, &selection).await;
            }
        }
    }

    Ok(())
}

async fn remove_connection_with_confirmation(
    ssh_service: &SshService,
    connection: &crate::models::Connection,
) -> Result<()> {
    println!("\n⚠️  WARNING: You are about to remove the following connection:");
    println!("   Name: {}", connection.name);
    println!("   Host: {}:{}", connection.host, connection.port);
    println!("   User: {}", connection.user);

    if !connection.tags.is_empty() {
        println!("   Tags: {}", connection.tags.join(", "));
    }

    print!(
        "\nType the connection name '{}' to confirm removal: ",
        connection.name
    );
    io::stdout().flush()?;

    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;
    let confirmation = confirmation.trim();

    if confirmation == connection.name {
        if ssh_service.remove_connection(&connection.name).await? {
            println!("✅ Connection '{}' removed successfully!", connection.name);
        } else {
            println!("❌ Failed to remove connection '{}'", connection.name);
        }
    } else {
        println!("❌ Confirmation failed. Removal cancelled.");
    }

    Ok(())
}

fn print_connection_info(connection: &crate::models::Connection, index: usize) {
    let tags_str = if connection.tags.is_empty() {
        "".to_string()
    } else {
        format!(" [{}]", connection.tags.join(", "))
    };

    let last_used = connection
        .last_used
        .map(|dt| format!(" (last used: {})", format_duration(dt)))
        .unwrap_or_default();

    println!("  {}. {} ({})", index, connection.name, connection.host);
    println!(
        "     Tags: {}{}",
        if tags_str.is_empty() {
            "none"
        } else {
            &tags_str[1..tags_str.len() - 1]
        },
        last_used
    );
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

fn interactive_selection(
    connections: &[crate::models::Connection],
    prompt: &str,
) -> Result<Option<crate::models::Connection>> {
    loop {
        print!(
            "{} [1-{}, 's' to search again, 'q' to quit]: ",
            prompt,
            connections.len()
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "q" | "quit" => {
                println!("Removal cancelled.");
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

                // For now, we'll just continue the loop
                println!("New search functionality would be implemented here.");
                continue;
            }
            _ => {
                if let Ok(index) = input.parse::<usize>() {
                    if index >= 1 && index <= connections.len() {
                        return Ok(Some(connections[index - 1].clone()));
                    } else {
                        println!(
                            "Invalid selection. Please enter a number between 1 and {}.",
                            connections.len()
                        );
                    }
                } else {
                    println!("Invalid input. Please enter a number, 's' to search again, or 'q' to quit.");
                }
            }
        }
    }
}
