use bayesian_ssh::services::sftp_shared::RemoteFileEntry;

#[tauri::command]
pub fn list_remote_directory(
    connection_name: String,
    remote_path: String,
) -> Result<Vec<RemoteFileEntry>, String> {
    bayesian_ssh::services::sftp_shared::list_remote_directory(&connection_name, &remote_path)
}
