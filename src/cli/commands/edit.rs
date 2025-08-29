use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::Result;
use std::io::{self, Write};
use tracing::info;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    target: String,
    name: Option<String>,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    info!("Editing connection: {}", target);

    let ssh_service = SshService::new(config)?;

    // First try exact match
    if let Some(connection) = ssh_service.get_connection(&target).await? {
        return update_connection(
            ssh_service,
            connection,
            name,
            host,
            user,
            port,
            kerberos,
            bastion,
            no_bastion,
            bastion_user,
            key,
            add_tags,
            remove_tags,
        )
        .await;
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
                    interactive_selection(&recent, "Select connection to edit")?
                {
                    return update_connection(
                        ssh_service,
                        selection,
                        name,
                        host,
                        user,
                        port,
                        kerberos,
                        bastion,
                        no_bastion,
                        bastion_user,
                        key,
                        add_tags,
                        remove_tags,
                    )
                    .await;
                }
            } else {
                println!("No recent connections found.");
                println!("Use 'bayesian-ssh add' to create a new connection.");
            }
        }
        1 => {
            // Single match - ask for confirmation
            let conn = &matches[0];
            println!("Found one similar connection:");
            print_connection_info(conn, 1);

            print!("Edit this connection? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();

            if matches!(input.as_str(), "y" | "yes") {
                return update_connection(
                    ssh_service,
                    conn.clone(),
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    add_tags,
                    remove_tags,
                )
                .await;
            } else {
                println!("Edit cancelled.");
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

            if let Some(selection) = interactive_selection(&matches, "Select connection to edit")? {
                return update_connection(
                    ssh_service,
                    selection,
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    add_tags,
                    remove_tags,
                )
                .await;
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_connection(
    ssh_service: SshService,
    mut connection: crate::models::Connection,
    name: Option<String>,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    no_bastion: bool,
    bastion_user: Option<String>,
    key: Option<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
) -> Result<()> {
    // Update fields if provided
    if let Some(name) = name {
        connection.name = name;
    }
    if let Some(host) = host {
        connection.host = host;
    }
    if let Some(user) = user {
        connection.user = user;
    }
    if let Some(port) = port {
        connection.port = port;
    }
    if let Some(kerberos) = kerberos {
        connection.use_kerberos = kerberos;
    }
    if let Some(bastion) = bastion {
        connection.bastion = Some(bastion);
    }
    if no_bastion {
        connection.bastion = None;
        connection.bastion_user = None;
    }
    if let Some(bastion_user) = bastion_user {
        connection.bastion_user = Some(bastion_user);
    }
    if let Some(key) = key {
        connection.key_path = Some(key);
    }

    // Handle tags
    for tag in add_tags {
        connection.add_tag(tag);
    }
    for tag in remove_tags {
        connection.remove_tag(&tag);
    }

    // Update in database
    ssh_service.update_connection(connection.clone()).await?;

    println!("✅ Connection '{}' updated successfully!", connection.name);
    println!("\nUpdated connection details:");
    println!("  Name: {}", connection.name);
    println!("  Host: {}:{}", connection.host, connection.port);
    println!("  User: {}", connection.user);
    if let Some(bastion) = &connection.bastion {
        println!(
            "  Bastion: {}@{}",
            connection
                .bastion_user
                .as_deref()
                .unwrap_or(&connection.user),
            bastion
        );
    }
    println!(
        "  Kerberos: {}",
        if connection.use_kerberos {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    if !connection.tags.is_empty() {
        println!("  Tags: {}", connection.tags.join(", "));
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
                println!("Edit cancelled.");
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
