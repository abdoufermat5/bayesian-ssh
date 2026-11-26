use super::{AllowedPaths, CommandError};
use bayesian_ssh::services::sftp_shared::RemoteFileEntry;
use std::path::Path;
use tauri::State;

// Spawns ssh/sftp against the remote host: run on Tokio's blocking pool.
#[tauri::command]
pub async fn list_remote_directory(
    connection_name: String,
    remote_path: String,
) -> Result<Vec<RemoteFileEntry>, CommandError> {
    tokio::task::spawn_blocking(move || {
        let config = super::load_config()?;
        bayesian_ssh::services::sftp_shared::list_remote_directory(
            &config,
            &connection_name,
            &remote_path,
        )
        .map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command]
pub async fn download_remote_file(
    allowed: State<'_, AllowedPaths>,
    connection_name: String,
    remote_path: String,
    local_path: String,
) -> Result<u64, CommandError> {
    let local = local_path.trim();
    if local.is_empty() {
        return Err("Local destination path cannot be empty.".into());
    }
    let verified_local = allowed.check_write_file(Path::new(local))?;
    let config = tokio::task::spawn_blocking(super::load_config)
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;
    bayesian_ssh::services::sftp_shared::download_remote_file(
        &config,
        &connection_name,
        &remote_path,
        &verified_local,
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn create_remote_directory(
    connection_name: String,
    remote_path: String,
) -> Result<(), CommandError> {
    let config = tokio::task::spawn_blocking(super::load_config)
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;
    bayesian_ssh::services::sftp_shared::create_remote_directory(
        &config,
        &connection_name,
        &remote_path,
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn rename_remote_entry(
    connection_name: String,
    from_path: String,
    to_path: String,
) -> Result<(), CommandError> {
    let config = tokio::task::spawn_blocking(super::load_config)
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;
    bayesian_ssh::services::sftp_shared::rename_remote_entry(
        &config,
        &connection_name,
        &from_path,
        &to_path,
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn delete_remote_entry(
    connection_name: String,
    remote_path: String,
    is_dir: bool,
) -> Result<(), CommandError> {
    let config = tokio::task::spawn_blocking(super::load_config)
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))??;
    bayesian_ssh::services::sftp_shared::delete_remote_entry(
        &config,
        &connection_name,
        &remote_path,
        is_dir,
    )
    .await
    .map_err(CommandError::from)
}
