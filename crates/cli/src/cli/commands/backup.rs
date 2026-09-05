use crate::config::AppConfig;
use crate::database::Database;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

pub async fn execute(output: Option<String>, config: AppConfig) -> Result<()> {
    let db_path = &config.database_path;

    if !db_path.exists() {
        anyhow::bail!("Database file does not exist at {:?}", db_path);
    }

    let backup_path = if let Some(path) = output {
        PathBuf::from(path)
    } else {
        let parent = db_path.parent().unwrap_or_else(|| std::path::Path::new(""));
        let backups_dir = parent.join("backups");
        fs::create_dir_all(&backups_dir).context("Failed to create backups directory")?;
        crate::config::enforce_secure_dir(&backups_dir);

        let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        backups_dir.join(format!("backup-{}.db", timestamp))
    };

    info!(
        "Backing up database from {:?} to {:?}",
        db_path, backup_path
    );

    // Use SQLite's online backup API (VACUUM INTO) instead of a raw
    // `fs::copy`. A running WAL-mode database can otherwise be copied
    // mid-checkpoint, producing an inconsistent snapshot. VACUUM INTO
    // produces a consistent, standalone copy.
    let db = Database::new(&config).context("open database for backup")?;
    db.vacuum_into(&backup_path)
        .context("backup database via VACUUM INTO")?;

    crate::config::enforce_secure_file(&backup_path);

    println!(
        "✅ Database successfully backed up to: {}",
        backup_path.display()
    );

    Ok(())
}
