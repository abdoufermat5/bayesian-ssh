//! Thin Tauri command wrappers over the shared Kerberos service.
//!
//! All logic lives in `bayesian_ssh::services::kerberos`; this module only
//! exposes the `#[tauri::command]` surface invoked from the Svelte frontend.

use bayesian_ssh::services::kerberos;

use crate::commands::CommandError;

// Spawns `klist` (polled periodically): run on Tokio's blocking pool.
#[tauri::command]
pub async fn get_kerberos_status() -> Result<kerberos::KerberosStatus, CommandError> {
    tokio::task::spawn_blocking(|| kerberos::status().map_err(CommandError::from))
        .await
        .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

// kinit talks to the KDC and can block for its full timeout: run on Tokio's
// blocking pool so async worker threads stay responsive.
#[tauri::command]
pub async fn renew_kerberos_ticket(
    password: Option<String>,
) -> Result<kerberos::KerberosStatus, CommandError> {
    tokio::task::spawn_blocking(move || {
        kerberos::renew_ticket(password).map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command]
pub async fn acquire_kerberos_ticket(
    principal: Option<String>,
    password: String,
    forwardable: Option<bool>,
    proxiable: Option<bool>,
    lifetime: Option<String>,
    renew_lifetime: Option<String>,
) -> Result<kerberos::KerberosStatus, CommandError> {
    // The principal is passed to kinit as a positional argument; a leading '-'
    // would be parsed as a kinit option instead (e.g. "-c/path" redirects the cache).
    if principal
        .as_deref()
        .is_some_and(|p| p.trim().starts_with('-'))
    {
        return Err("Principal cannot start with '-'".into());
    }
    tokio::task::spawn_blocking(move || {
        kerberos::acquire_ticket(
            principal,
            password,
            forwardable,
            proxiable,
            lifetime,
            renew_lifetime,
        )
        .map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}
