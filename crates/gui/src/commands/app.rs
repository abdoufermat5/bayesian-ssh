use serde::Serialize;
use tauri::AppHandle;

use crate::tray;

#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
    tray::quit_application(&app);
    Ok(())
}

#[tauri::command]
pub fn force_quit_app(app: AppHandle) -> Result<(), String> {
    tray::force_quit_application(&app);
    Ok(())
}

#[derive(Serialize)]
pub struct EnvStatus {
    pub ssh_agent_available: bool,
    pub ssh_auth_sock: Option<String>,
    pub kerberos_available: bool,
    pub krb5_ccname: Option<String>,
    pub warnings: Vec<String>,
}

/// Returns the current SSH agent / Kerberos environment status so the frontend
/// can display a dismissible warning when these are missing.
#[tauri::command]
pub fn get_env_status() -> EnvStatus {
    let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK").ok();
    let krb5_ccname = std::env::var("KRB5CCNAME").ok();

    let ssh_agent_available = ssh_auth_sock.as_deref().is_some_and(|s| {
        std::path::Path::new(s).exists()
    });

    let kerberos_available = krb5_ccname.is_some();

    let mut warnings = Vec::new();
    if !ssh_agent_available {
        warnings.push(
            "No SSH agent socket found. Key-based authentication may fail. \
             Try running `eval $(ssh-agent)` and `ssh-add` in a terminal, or ensure \
             your SSH agent is started at login."
                .to_string(),
        );
    }

    EnvStatus {
        ssh_agent_available,
        ssh_auth_sock,
        kerberos_available,
        krb5_ccname,
        warnings,
    }
}
