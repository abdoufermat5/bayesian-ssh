use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::database::Database;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

pub async fn execute(file: String, force: bool, config: AppConfig) -> Result<()> {
    let restore_path = PathBuf::from(&file);

    if !restore_path.exists() {
        bail!("Backup file does not exist: {}", file);
    }

    if !restore_path.is_file() {
        bail!("Path is not a file: {}", file);
    }

    // Reject non-database / corrupt files before touching the live database.
    validate_backup(&restore_path)?;

    let db_path = &config.database_path;

    if let (Ok(a), Ok(b)) = (restore_path.canonicalize(), db_path.canonicalize()) {
        if a == b {
            bail!("Refusing to restore the active database onto itself");
        }
    }

    if !force {
        println!("⚠️ WARNING: This will overwrite your current connection database!");
        println!("Current database: {}", db_path.display());
        println!("Restore file:     {}", restore_path.display());
        println!();

        if !confirm("Are you sure you want to proceed?", false)? {
            println!("Restore cancelled.");
            return Ok(());
        }
    }

    info!(
        "Restoring database from {:?} to {:?}",
        restore_path, db_path
    );

    let parent = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).context("Failed to create database directory")?;

    // Safety backup before overwriting. A consistent SQLite snapshot
    // (VACUUM INTO) rather than a raw file copy; if it cannot be taken the
    // restore is aborted instead of risking unrecoverable data loss.
    if db_path.exists() {
        let backups_dir = parent.join("backups");
        fs::create_dir_all(&backups_dir).context("Failed to create backups directory")?;
        crate::config::enforce_secure_dir(&backups_dir);
        let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        let safety_backup = backups_dir.join(format!("pre-restore-{}.db", timestamp));

        Database::new(&config)
            .and_then(|db| db.vacuum_into(&safety_backup))
            .context("Failed to create safety backup; restore aborted")?;
        crate::config::enforce_secure_file(&safety_backup);
        println!("Created safety backup at: {}", safety_backup.display());
    }

    // Copy into a 0600 temp file next to the database, then atomically
    // rename it over the live file: a failed copy never leaves a truncated
    // database, and `fs::copy`'s permission copying cannot widen access.
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .context("Failed to create temporary restore file")?;
    let mut src = fs::File::open(&restore_path).context("Failed to open backup file")?;
    std::io::copy(&mut src, tmp.as_file_mut()).context("Failed to copy backup file")?;
    tmp.as_file().sync_all()?;
    tmp.persist(db_path)
        .context("Failed to restore database file")?;

    // Sidecar files belong to the replaced database; SQLite would otherwise
    // replay a stale journal/WAL onto the restored file.
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = db_path.as_os_str().to_owned();
        sidecar.push(suffix);
        let _ = fs::remove_file(PathBuf::from(sidecar));
    }

    // Opening runs schema migrations so older backups are upgraded now.
    Database::new(&config).context("Restored database could not be opened")?;

    println!(
        "✅ Database successfully restored from {}",
        restore_path.display()
    );

    Ok(())
}

/// Check that `path` is an intact SQLite database containing the
/// bayesian-ssh `connections` table. Opened read-only so validation can never
/// modify (or create) the file.
fn validate_backup(path: &Path) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};

    let not_db = || format!("{} is not a valid SQLite database", path.display());
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).with_context(not_db)?;
    let check: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .with_context(not_db)?;
    if check != "ok" {
        bail!(
            "{} failed the SQLite integrity check: {}",
            path.display(),
            check
        );
    }
    let has_connections: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'connections')",
            [],
            |r| r.get(0),
        )
        .with_context(not_db)?;
    if !has_connections {
        bail!(
            "{} is not a bayesian-ssh database (no 'connections' table)",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_backup;
    use crate::config::AppConfig;
    use crate::database::Database;

    #[test]
    fn rejects_non_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "definitely not sqlite").unwrap();
        assert!(validate_backup(&path).is_err());
    }

    #[test]
    fn rejects_foreign_sqlite_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER);")
            .unwrap();
        assert!(validate_backup(&path).is_err());
    }

    #[test]
    fn accepts_bayesian_ssh_database() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("history.db"),
            ..Default::default()
        };
        drop(Database::new(&config).unwrap());
        assert!(validate_backup(&config.database_path).is_ok());
    }
}
