//! Thin Tauri command wrappers over the shared Kerberos service.
//!
//! All logic lives in `bayesian_ssh::services::kerberos`; this module only
//! exposes the `#[tauri::command]` surface invoked from the Svelte frontend.

use bayesian_ssh::services::kerberos;

use crate::commands::CommandError;

// Spawns `klist` (polled periodically): keep it off the main/UI thread.
#[tauri::command(async)]
pub fn get_kerberos_status() -> Result<kerberos::KerberosStatus, CommandError> {
    kerberos::get_status().map_err(CommandError::from)
}

// kinit talks to the KDC and can block for its full timeout: run these off the
// main thread so the UI stays responsive.
#[tauri::command(async)]
pub fn renew_kerberos_ticket(
    password: Option<String>,
) -> Result<kerberos::KerberosStatus, CommandError> {
    kerberos::renew_ticket(password).map_err(CommandError::from)
}

#[tauri::command(async)]
pub fn acquire_kerberos_ticket(
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
    kerberos::acquire_ticket(
        principal,
        password,
        forwardable,
        proxiable,
        lifetime,
        renew_lifetime,
    )
    .map_err(CommandError::from)
}
