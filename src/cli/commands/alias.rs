//! Alias command implementation - manage connection aliases

use crate::config::AppConfig;
use crate::database::Database;
use crate::cli::utils::fuzzy_select_connection;
use crate::services::SshService;
use anyhow::{bail, Result};

/// Execute the alias command
pub async fn execute(
    action: AliasAction,
    config: AppConfig,
) -> Result<()> {
    let db = Database::new(&config)?;

    match action {
        AliasAction::Add { alias, target } => {
            add_alias(&db, &alias, &target, &config).await?;
        }
        AliasAction::Remove { alias } => {
            remove_alias(&db, &alias)?;
        }
        AliasAction::List { target } => {
            list_aliases(&db, target.as_deref(), &config).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub enum AliasAction {
    Add { alias: String, target: String },
    Remove { alias: String },
    List { target: Option<String> },
}

async fn add_alias(db: &Database, alias: &str, target: &str, config: &AppConfig) -> Result<()> {
    // Check if alias already exists
    if let Some(existing) = db.get_connection_by_alias(alias)? {
        bail!(
            "Alias '{}' already exists and points to '{}'",
            alias,
            existing.name
        );
    }

    // Check if alias conflicts with an existing connection name
    if db.get_connection(alias)?.is_some() {
        bail!(
            "Cannot create alias '{}' - a connection with that name already exists",
            alias
        );
    }

    // Find the target connection
    let ssh_service = SshService::new(config.clone())?;
    let connection = if let Some(conn) = db.get_connection(target)? {
        conn
    } else {
        // Try fuzzy search
        match fuzzy_select_connection(&ssh_service, target, "alias", true).await? {
            Some(conn) => conn,
            None => {
                bail!("No connection found matching '{}'", target);
            }
        }
    };

    // Add the alias
    db.add_alias(alias, &connection.id.to_string())?;

    println!("✅ Added alias '{}' → '{}'", alias, connection.name);
    println!("   You can now use: bssh connect {}", alias);

    Ok(())
}

fn remove_alias(db: &Database, alias: &str) -> Result<()> {
    if db.remove_alias(alias)? {
        println!("✅ Removed alias '{}'", alias);
    } else {
        println!("❌ Alias '{}' not found", alias);
    }
    Ok(())
}

async fn list_aliases(db: &Database, target: Option<&str>, config: &AppConfig) -> Result<()> {
    if let Some(target_name) = target {
        // List aliases for a specific connection
        let ssh_service = SshService::new(config.clone())?;
        let connection = if let Some(conn) = db.get_connection(target_name)? {
            conn
        } else {
            match fuzzy_select_connection(&ssh_service, target_name, "show aliases for", true).await? {
                Some(conn) => conn,
                None => {
                    bail!("No connection found matching '{}'", target_name);
                }
            }
        };

        let aliases = db.get_aliases_for_connection(&connection.id.to_string())?;

        if aliases.is_empty() {
            println!("📝 No aliases for connection '{}'", connection.name);
            println!("   Use 'bssh alias add <alias> {}' to create one", connection.name);
        } else {
            println!("📝 Aliases for '{}':", connection.name);
            for alias in &aliases {
                println!("   • {}", alias);
            }
        }
    } else {
        // List all aliases
        let connections = db.list_connections(None, false)?;
        let mut found_any = false;

        println!("📝 All Connection Aliases\n");

        for conn in connections {
            let aliases = db.get_aliases_for_connection(&conn.id.to_string())?;
            if !aliases.is_empty() {
                found_any = true;
                println!("  {} ({})", conn.name, conn.host);
                for alias in &aliases {
                    println!("    └─ {}", alias);
                }
                println!();
            }
        }

        if !found_any {
            println!("  No aliases defined yet.");
            println!("\n  Use 'bssh alias add <alias> <connection>' to create aliases.");
        }
    }

    Ok(())
}
