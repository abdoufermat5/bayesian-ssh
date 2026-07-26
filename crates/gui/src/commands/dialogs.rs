#[tauri::command]
pub fn pick_key_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let ssh_dir = dirs::home_dir().map(|h| h.join(".ssh"));

    let mut dialog = rfd::FileDialog::new()
        .set_title("Select SSH Private Key")
        .set_parent(&window);

    if let Some(ref path) = ssh_dir {
        if path.exists() {
            dialog = dialog.set_directory(path);
        }
    }

    let _ = window.minimize();
    let file = dialog.pick_file();
    let _ = window.unminimize();
    let _ = window.set_focus();

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn pick_ssh_config_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let ssh_dir = dirs::home_dir().map(|h| h.join(".ssh"));

    let mut dialog = rfd::FileDialog::new()
        .set_title("Select SSH Config File")
        .add_filter("SSH Config", &["config", "conf"])
        .set_parent(&window);

    if let Some(ref path) = ssh_dir {
        if path.exists() {
            dialog = dialog.set_directory(path);
        }
    }

    let _ = window.minimize();
    let file = dialog.pick_file();
    let _ = window.unminimize();
    let _ = window.set_focus();

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn save_backup_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export Bayesian SSH Backup")
        .set_file_name("bayesian-ssh-backup.json")
        .add_filter("JSON Backup (*.json)", &["json"])
        .add_filter("Encrypted Backup (*.enc)", &["enc"])
        .add_filter("All Files", &["*"])
        .set_parent(&window);

    if let Some(home) = dirs::home_dir() {
        dialog = dialog.set_directory(home);
    }

    let _ = window.minimize();
    let file = dialog.save_file();
    let _ = window.unminimize();
    let _ = window.set_focus();

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn pick_backup_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Select Bayesian SSH Backup File")
        .add_filter("Backup Files (*.json, *.enc)", &["json", "enc", "bak"])
        .add_filter("All Files", &["*"])
        .set_parent(&window);

    if let Some(home) = dirs::home_dir() {
        dialog = dialog.set_directory(home);
    }

    let _ = window.minimize();
    let file = dialog.pick_file();
    let _ = window.unminimize();
    let _ = window.set_focus();

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}
