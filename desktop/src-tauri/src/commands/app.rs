use tauri::AppHandle;

use crate::tray;

#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
    tray::quit_application(&app);
    Ok(())
}
