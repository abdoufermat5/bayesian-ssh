//! Shared CLI utilities for interactive selection and fuzzy search display.
//!
//! This module provides common UI components used across multiple commands
//! to reduce code duplication and ensure consistent UX.

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::SshService;
use crate::ui::{Glyph, KvList, Style, Tone, heading};
use anyhow::Result;
use std::io::{self, IsTerminal, Write};

/// Upper bound on concurrent per-host tasks for multi-host commands
/// (`exec`, `ping`) so `--all` over a large inventory cannot exhaust
/// file descriptors or spawn hundreds of `ssh` processes at once.
pub const MAX_PARALLEL_HOSTS: usize = 16;

fn require_interactive_stdin(context: &str) -> Result<()> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("Interactive terminal required for {context} (stdin is not a TTY)");
    }
    Ok(())
}

/// Read one trimmed, lowercased line from stdin. Returns `None` on EOF so
/// prompts never loop forever (or silently accept a default) when stdin is
/// closed or not a terminal.
fn read_answer() -> Result<Option<String>> {
    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        return Ok(None);
    }
    Ok(Some(input.trim().to_lowercase()))
}

/// Truncate `s` to at most `max_cols` display columns (appending `…` when cut).
/// Uses Unicode display width so multi-byte and wide characters fit cleanly.
pub fn truncate_display(s: &str, max_cols: usize) -> String {
    crate::ui::truncate(s, max_cols)
}

/// Format a byte count as a human-readable string (B/KiB/MiB/GiB/TiB).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let b = bytes as f64;
    if b >= TIB {
        format!("{:.1} TiB", b / TIB)
    } else if b >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Format an elapsed duration compactly (e.g. "42s", "3m 5s", "2h 10m").
pub fn format_elapsed(duration: chrono::Duration) -> String {
    let secs = duration.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

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

/// Display connection info in a consistent format for interactive selection lists.
pub fn print_connection_info(connection: &Connection, index: usize) {
    let style = Style::stderr();
    let last_used = connection
        .last_used
        .map(|dt| format!("last used {}", format_duration(dt)))
        .unwrap_or_default();
    let tags = if connection.tags.is_empty() {
        String::new()
    } else {
        format!("tags: {}", connection.tags.join(", "))
    };
    let meta = style.facts(&[&tags, &last_used]);

    eprintln!(
        "  {} {} {}",
        style.paint(Tone::Dim, &format!("{index}.")),
        style.paint(Tone::Strong, &connection.name),
        style.paint(Tone::Dim, &format!("({})", connection.host))
    );
    if !meta.is_empty() {
        eprintln!("     {}", style.paint(Tone::Dim, &meta));
    }
}

/// Result type for interactive selection operations
pub enum SelectionResult {
    /// User selected a connection
    Selected(Box<Connection>),
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
    require_interactive_stdin("selection")?;
    let style = Style::stderr();

    loop {
        eprint!(
            "{} {}: ",
            prompt,
            style.paint(
                Tone::Dim,
                &format!(
                    "[1-{}, 's' to search again, 'q' to quit]",
                    connections.len()
                ),
            )
        );
        io::stderr().flush()?;

        let Some(input) = read_answer()? else {
            return Ok(SelectionResult::Cancelled);
        };

        match input.as_str() {
            "q" | "quit" => {
                return Ok(SelectionResult::Cancelled);
            }
            "s" | "search" => {
                eprint!("Enter new search term: ");
                io::stderr().flush()?;

                let mut new_search = String::new();
                if io::stdin().read_line(&mut new_search)? == 0 {
                    return Ok(SelectionResult::Cancelled);
                }
                let new_search = new_search.trim().to_string();

                if new_search.is_empty() {
                    eprintln!(
                        "{} Empty search term. Please try again.",
                        style.mark(Glyph::Bad)
                    );
                    continue;
                }

                return Ok(SelectionResult::SearchAgain(new_search));
            }
            _ => {
                if let Ok(index) = input.parse::<usize>() {
                    if index >= 1 && index <= connections.len() {
                        return Ok(SelectionResult::Selected(Box::new(
                            connections[index - 1].clone(),
                        )));
                    } else {
                        eprintln!(
                            "{} Invalid selection. Please enter a number between 1 and {}.",
                            style.mark(Glyph::Bad),
                            connections.len()
                        );
                    }
                } else {
                    eprintln!(
                        "{} Invalid input. Enter a number, 's' to search again, or 'q' to quit.",
                        style.mark(Glyph::Bad)
                    );
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
/// Uses `config.search_mode`, so callers must pass the config of the active
/// (possibly `--env`-overridden) environment.
///
/// Returns the selected connection, or None if cancelled.
pub async fn fuzzy_select_connection(
    ssh_service: &SshService,
    initial_query: &str,
    action_name: &str,
    auto_select_single: bool,
    config: &AppConfig,
) -> Result<Option<Connection>> {
    let style = Style::stderr();
    let mut current_query = initial_query.to_string();
    let search_mode = config.search_mode.as_str();

    loop {
        let matches = ssh_service.search(&current_query, 10, search_mode).await?;

        match matches.len() {
            0 => {
                eprintln!(
                    "{} No connections found matching '{}'",
                    style.mark(Glyph::Bad),
                    current_query
                );
                eprintln!(
                    "  {} Run `bssh list` to see all connections or `bssh add` to create one.",
                    style.paint(Tone::Info, "hint:")
                );

                // Show recent connections as fallback when interactive
                let recent = ssh_service.recent_connections(5).await?;
                if !recent.is_empty() {
                    require_interactive_stdin("connection selection")?;
                    eprintln!("\n{}", style.label("Recent connections"));
                    for (i, conn) in recent.iter().enumerate() {
                        let last_used = conn
                            .last_used
                            .map(|dt| format!(" (last: {})", format_duration(dt)))
                            .unwrap_or_default();
                        eprintln!(
                            "  {} {}{}",
                            style.paint(Tone::Dim, &format!("{}.", i + 1)),
                            style.paint(Tone::Strong, &conn.name),
                            style.paint(Tone::Dim, &last_used)
                        );
                    }

                    match interactive_selection_with_search(
                        &recent,
                        &format!("Select recent connection to {action_name}"),
                    )? {
                        SelectionResult::Selected(conn) => return Ok(Some(*conn)),
                        SelectionResult::SearchAgain(new_query) => {
                            current_query = new_query;
                            continue;
                        }
                        SelectionResult::Cancelled => {
                            eprintln!("Operation cancelled.");
                            return Ok(None);
                        }
                    }
                } else {
                    return Ok(None);
                }
            }
            1 if auto_select_single => {
                // Single match with auto-select enabled - connect directly
                let conn = &matches[0];
                eprintln!(
                    "{} Matched {} {}",
                    style.mark(Glyph::Ok),
                    style.paint(Tone::Strong, &conn.name),
                    style.paint(Tone::Dim, &format!("({})", conn.host))
                );
                return Ok(Some(conn.clone()));
            }
            1 => {
                // Single match without auto-select - ask for confirmation
                require_interactive_stdin("connection confirmation")?;
                let conn = &matches[0];
                eprintln!("Found one similar connection:");
                print_connection_info(conn, 1);

                if confirm(
                    &format!("{} this connection?", capitalize_first(action_name)),
                    true,
                )? {
                    return Ok(Some(conn.clone()));
                } else {
                    eprintln!("Operation cancelled.");
                    return Ok(None);
                }
            }
            _ => {
                // Multiple matches - interactive selection
                require_interactive_stdin("connection selection")?;
                eprintln!("Found {} matches for '{}':", matches.len(), current_query);

                for (i, conn) in matches.iter().enumerate() {
                    print_connection_info(conn, i + 1);
                }

                match interactive_selection_with_search(
                    &matches,
                    &format!("Select connection to {action_name}"),
                )? {
                    SelectionResult::Selected(conn) => return Ok(Some(*conn)),
                    SelectionResult::SearchAgain(new_query) => {
                        current_query = new_query;
                        eprintln!();
                        continue;
                    }
                    SelectionResult::Cancelled => {
                        eprintln!("Operation cancelled.");
                        return Ok(None);
                    }
                }
            }
        }
    }
}

/// Capitalize the first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Ask for simple yes/no confirmation on stderr.
pub fn confirm(prompt: &str, default_yes: bool) -> Result<bool> {
    require_interactive_stdin("confirmation")?;
    let style = Style::stderr();
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    eprint!("{} {}: ", prompt, style.paint(Tone::Dim, hint));
    io::stderr().flush()?;

    let Some(input) = read_answer()? else {
        // EOF: never treat a closed stdin as consent.
        return Ok(false);
    };

    if input.is_empty() {
        return Ok(default_yes);
    }

    Ok(matches!(input.as_str(), "y" | "yes"))
}

/// Display detailed connection information
pub fn show_connection_details(connection: &Connection) -> Result<()> {
    let style = Style::terminal();
    let meta = format!("id {}", connection.id);
    println!("{}\n", heading(&style, &connection.name, Some(&meta)));

    let mut kv = KvList::new();
    kv.push("Host", format!("{}:{}", connection.host, connection.port));
    kv.push("User", connection.user.clone());

    if let Some(bastion) = &connection.bastion {
        kv.push(
            "Bastion",
            format!(
                "{}@{}",
                connection
                    .bastion_user
                    .as_deref()
                    .unwrap_or(&connection.user),
                bastion
            ),
        );
    }

    kv.push(
        "Kerberos",
        if connection.use_kerberos {
            style.paint(Tone::Ok, "Enabled")
        } else {
            style.paint(Tone::Dim, "Disabled")
        },
    );

    if let Some(key) = &connection.key_path {
        kv.push("SSH Key", key.clone());
    }

    kv.push(
        "Agent forwarding",
        if connection.forward_agent {
            style.paint(Tone::Warn, "Enabled")
        } else {
            style.paint(Tone::Dim, "Disabled")
        },
    );

    kv.push(
        "Created",
        connection
            .created_at
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    );

    if let Some(last_used) = connection.last_used {
        kv.push(
            "Last used",
            last_used.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        );
    }

    if !connection.tags.is_empty() {
        kv.push("Tags", connection.tags.join(", "));
    }

    if !connection.aliases.is_empty() {
        kv.push("Aliases", connection.aliases.join(", "));
    }

    kv.push("SSH Command", connection.to_ssh_command());

    println!("{}", kv.render(&style));

    if let Some(change) = &connection.pending_change {
        println!(
            "\n  {} {}",
            style.mark(Glyph::Ask),
            style.paint(
                Tone::Warn,
                &format!(
                    "Pending team sync change ({}):",
                    change.detected_at.format("%Y-%m-%d %H:%M:%S UTC")
                ),
            )
        );
        for line in crate::services::pending::describe(connection) {
            println!("    {line}");
        }
        println!(
            "    {}",
            style.paint(
                Tone::Dim,
                &format!("Confirm with `bssh confirm {}`", connection.name),
            )
        );
    }

    Ok(())
}

/// Resolve a connection by exact lookup or interactive fuzzy search.
///
/// Bails with "No connection selected" if the search is cancelled or no matches are found.
pub async fn resolve_connection(
    ssh_service: &SshService,
    target: &str,
    action: &str,
    auto_select_single: bool,
    config: &AppConfig,
) -> Result<Connection> {
    if let Some(conn) = ssh_service.connection(target).await? {
        return Ok(conn);
    }

    if let Some(conn) =
        fuzzy_select_connection(ssh_service, target, action, auto_select_single, config).await?
    {
        Ok(conn)
    } else {
        anyhow::bail!("No connection selected")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_display_is_width_safe() {
        assert_eq!(truncate_display("short", 10), "short");
        assert_eq!(truncate_display("abcdef", 4), "abc…");
        // Multi-byte and wide CJK chars fit within column width without panicking.
        assert_eq!(truncate_display("éééééé", 4), "ééé…");
        assert_eq!(truncate_display("服务器生产环境", 3), "服…");
        assert_eq!(truncate_display("服务器生产环境", 5), "服务…");
    }

    #[test]
    fn format_elapsed_buckets() {
        assert_eq!(format_elapsed(chrono::Duration::seconds(42)), "42s");
        assert_eq!(format_elapsed(chrono::Duration::seconds(185)), "3m 5s");
        assert_eq!(format_elapsed(chrono::Duration::seconds(7800)), "2h 10m");
    }
}
