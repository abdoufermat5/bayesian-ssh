//! Thin Tauri command wrappers over the shared Kerberos service.
//!
//! All logic lives in `bayesian_ssh::services::kerberos`; this module only
//! exposes the `#[tauri::command]` surface invoked from the Svelte frontend.

use bayesian_ssh::services::kerberos;

#[tauri::command]
pub fn get_kerberos_status() -> Result<kerberos::KerberosStatus, String> {
    kerberos::get_status()
}

#[tauri::command]
pub fn renew_kerberos_ticket(password: Option<String>) -> Result<kerberos::KerberosStatus, String> {
    kerberos::renew_ticket(password)
}

#[tauri::command]
pub fn acquire_kerberos_ticket(
    principal: Option<String>,
    password: String,
    forwardable: Option<bool>,
    proxiable: Option<bool>,
    lifetime: Option<String>,
    renew_lifetime: Option<String>,
) -> Result<kerberos::KerberosStatus, String> {
    kerberos::acquire_ticket(principal, password, forwardable, proxiable, lifetime, renew_lifetime)
}
