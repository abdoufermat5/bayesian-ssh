use crate::config::AppConfig;
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
        
        let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        backups_dir.join(format!("backup-{}.db", timestamp))
    };

    info!("Backing up database from {:?} to {:?}", db_path, backup_path);
    
    fs::copy(db_path, &backup_path).context("Failed to copy database file")?;
    
    println!("✅ Database successfully backed up to: {}", backup_path.display());
    
    Ok(())
}
