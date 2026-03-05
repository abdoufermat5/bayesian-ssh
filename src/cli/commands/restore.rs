use crate::cli::utils::confirm;
use crate::config::AppConfig;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

pub async fn execute(file: String, force: bool, config: AppConfig) -> Result<()> {
    let restore_path = PathBuf::from(&file);

    if !restore_path.exists() {
        anyhow::bail!("Backup file does not exist: {}", file);
    }

    if !restore_path.is_file() {
        anyhow::bail!("Path is not a file: {}", file);
    }

    let default_db_path = &config.database_path;

    if !force {
        println!("⚠️ WARNING: This will overwrite your current connection database!");
        println!("Current database: {}", default_db_path.display());
        println!("Restore file:     {}", restore_path.display());
        println!();

        if !confirm("Are you sure you want to proceed?", false)? {
            println!("Restore cancelled.");
            return Ok(());
        }
    }

    info!(
        "Restoring database from {:?} to {:?}",
        restore_path, default_db_path
    );

    // Create a safety backup before overwriting
    if default_db_path.exists() {
        let parent = default_db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let backups_dir = parent.join("backups");
        let _ = fs::create_dir_all(&backups_dir);
        let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        let safety_backup = backups_dir.join(format!("pre-restore-{}.db", timestamp));

        if let Err(e) = fs::copy(default_db_path, &safety_backup) {
            info!("Failed to create safety backup: {}", e);
        } else {
            println!("Created safety backup at: {}", safety_backup.display());
        }
    }

    fs::copy(&restore_path, default_db_path).context("Failed to restore database file")?;

    println!(
        "✅ Database successfully restored from {}",
        restore_path.display()
    );

    Ok(())
}
