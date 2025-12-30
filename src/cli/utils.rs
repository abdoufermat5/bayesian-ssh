//! Shared CLI utilities for interactive selection and fuzzy search display.
//!
//! This module provides common UI components used across multiple commands
//! to reduce code duplication and ensure consistent UX.

use crate::models::Connection;
use crate::services::SshService;
use anyhow::Result;
use std::io::{self, Write};

/// Format a datetime as a human-readable duration string (e.g., "2 hours ago")
pub fn format_duration(dt: chrono::DateTime<chrono::Utc>) -> String {
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

/// Display connection info in a consistent format for selection lists
pub fn print_connection_info(connection: &Connection, index: usize) {
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

/// Result type for interactive selection operations
pub enum SelectionResult {
    /// User selected a connection
    Selected(Connection),
    /// User wants to search again with a new query
    SearchAgain(String),
    /// User cancelled the operation
    Cancelled,
}

/// Interactive selection with support for search-again functionality.
///
/// Returns `SelectionResult` which can indicate:
/// - A selected connection
/// - A request to search again with a new query
/// - A cancellation
pub fn interactive_selection_with_search(
    connections: &[Connection],
    prompt: &str,
) -> Result<SelectionResult> {
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
                return Ok(SelectionResult::Cancelled);
            }
            "s" | "search" => {
                print!("Enter new search term: ");
                io::stdout().flush()?;

                let mut new_search = String::new();
                io::stdin().read_line(&mut new_search)?;
                let new_search = new_search.trim().to_string();

                if new_search.is_empty() {
                    println!("❌ Empty search term. Please try again.");
                    continue;
                }

                return Ok(SelectionResult::SearchAgain(new_search));
            }
            _ => {
                if let Ok(index) = input.parse::<usize>() {
                    if index >= 1 && index <= connections.len() {
                        return Ok(SelectionResult::Selected(connections[index - 1].clone()));
                    } else {
                        println!(
                            "❌ Invalid selection. Please enter a number between 1 and {}.",
                            connections.len()
                        );
                    }
                } else {
                    println!("❌ Invalid input. Please enter a number, 's' to search again, or 'q' to quit.");
                }
            }
        }
    }
}

/// Display fuzzy search results with interactive selection.
///
/// This function handles:
/// - Displaying matching connections
/// - Interactive selection
/// - "Search again" functionality with recursive search
/// - Auto-connect for single matches (improved UX)
///
/// Returns the selected connection, or None if cancelled.
pub async fn fuzzy_select_connection(
    ssh_service: &SshService,
    initial_query: &str,
    action_name: &str,
    auto_select_single: bool,
) -> Result<Option<Connection>> {
    let mut current_query = initial_query.to_string();

    loop {
        let matches = ssh_service.fuzzy_search(&current_query, 10).await?;

        match matches.len() {
            0 => {
                println!("❌ No connections found matching '{}'", current_query);
                println!("\n💡 Tip: Use 'bssh list' to see all connections or 'bssh add' to create a new one.");

                // Show recent connections as fallback
                let recent = ssh_service.get_recent_connections(5).await?;
                if !recent.is_empty() {
                    println!("\n📅 Recent connections:");
                    for (i, conn) in recent.iter().enumerate() {
                        let last_used = conn
                            .last_used
                            .map(|dt| format!(" (last: {})", format_duration(dt)))
                            .unwrap_or_default();
                        println!("  {}. {}{}", i + 1, conn.name, last_used);
                    }

                    match interactive_selection_with_search(&recent, &format!("Select recent connection to {}", action_name))? {
                        SelectionResult::Selected(conn) => return Ok(Some(conn)),
                        SelectionResult::SearchAgain(new_query) => {
                            current_query = new_query;
                            continue;
                        }
                        SelectionResult::Cancelled => {
                            println!("Operation cancelled.");
                            return Ok(None);
                        }
                    }
                } else {
                    println!("No recent connections found.");
                    return Ok(None);
                }
            }
            1 if auto_select_single => {
                // Single match with auto-select enabled - connect directly
                let conn = &matches[0];
                println!("🔍 Found exact match: {} ({})", conn.name, conn.host);
                return Ok(Some(conn.clone()));
            }
            1 => {
                // Single match without auto-select - ask for confirmation
                let conn = &matches[0];
                println!("🔍 Found one similar connection:");
                print_connection_info(conn, 1);

                print!("{} this connection? [Y/n]: ", capitalize_first(action_name));
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let input = input.trim().to_lowercase();

                // Default to yes (Y is uppercase in prompt)
                if input.is_empty() || matches!(input.as_str(), "y" | "yes") {
                    return Ok(Some(conn.clone()));
                } else {
                    println!("Operation cancelled.");
                    return Ok(None);
                }
            }
            _ => {
                // Multiple matches - interactive selection
                println!(
                    "🔍 Found {} similar connections for '{}':",
                    matches.len(),
                    current_query
                );
                println!();

                for (i, conn) in matches.iter().enumerate() {
                    print_connection_info(conn, i + 1);
                }

                match interactive_selection_with_search(&matches, &format!("Select connection to {}", action_name))? {
                    SelectionResult::Selected(conn) => return Ok(Some(conn)),
                    SelectionResult::SearchAgain(new_query) => {
                        current_query = new_query;
                        println!(); // Add spacing before new results
                        continue;
                    }
                    SelectionResult::Cancelled => {
                        println!("Operation cancelled.");
                        return Ok(None);
                    }
                }
            }
        }
    }
}

/// Show "no matches" message with helpful suggestions
pub fn show_no_matches_message(query: &str) {
    println!("❌ No connections found matching '{}'", query);
    println!("\n💡 Suggestions:");
    println!("   • Use 'bssh list' to see all saved connections");
    println!("   • Use 'bssh add <name> <host>' to create a new connection");
    println!("   • Use 'bssh import' to import from ~/.ssh/config");
}

/// Capitalize the first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Ask for simple yes/no confirmation
pub fn confirm(prompt: &str, default_yes: bool) -> Result<bool> {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{} {}: ", prompt, hint);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input.is_empty() {
        return Ok(default_yes);
    }

    Ok(matches!(input.as_str(), "y" | "yes"))
}

/// Display detailed connection information
pub fn show_connection_details(connection: &Connection) -> Result<()> {
    println!("🔗 Connection Details: {}\n", connection.name);
    println!("  ID:       {}", connection.id);
    println!("  Host:     {}:{}", connection.host, connection.port);
    println!("  User:     {}", connection.user);

    if let Some(bastion) = &connection.bastion {
        println!(
            "  Bastion:  {}@{}",
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

    if let Some(key) = &connection.key_path {
        println!("  SSH Key:  {}", key);
    }

    println!(
        "  Created:  {}",
        connection.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    if let Some(last_used) = connection.last_used {
        println!("  Last used: {}", last_used.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    if !connection.tags.is_empty() {
        println!("  Tags:     {}", connection.tags.join(", "));
    }

    if !connection.aliases.is_empty() {
        println!("  Aliases:  {}", connection.aliases.join(", "));
    }

    println!("\n  SSH Command: {}", connection.to_ssh_command());

    Ok(())
}
