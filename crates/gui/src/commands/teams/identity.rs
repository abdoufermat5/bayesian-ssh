//! Teams identity: status, init, e-mail code, unlock and lock.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use super::dto::{EmailCodeDto, IdentityDto, TeamDto, TeamsStatus};
use super::state::{
    block_on, command_error, on_init_thread, on_teams_thread, open_store, TeamsInitError,
    TeamsState,
};
use crate::commands::CommandError;
use bayesian_ssh::services::teams::client::TeamsError;
use bayesian_ssh::services::teams::crypto::fingerprint;
use bayesian_ssh::services::teams::session::{self};
use tauri::State;
use zeroize::Zeroizing;

// ── Commands ────────────────────────────────────────────────────────────────

pub(super) fn status(state: &TeamsState) -> anyhow::Result<TeamsStatus> {
    let (store, _) = open_store()?;
    let db = store.database();
    let identity = session::load_identity(db)?;
    let keys = state.get();
    let summaries = db.list_sources()?;
    let teams = session::local_teams(db)?
        .into_iter()
        .map(|(team_id, source)| TeamDto {
            team_id: team_id.to_string(),
            source_id: source.id.to_string(),
            hosts: summaries
                .iter()
                .find(|s| s.source.id == source.id)
                .map_or(0, |s| s.hosts),
            last_sync_at: source.last_sync_at,
            name: source.name,
        })
        .collect();
    Ok(TeamsStatus {
        unlocked: keys.is_some(),
        fingerprint: keys.map(|k| fingerprint(&k.ed25519_public())),
        identity: identity.map(|i| IdentityDto {
            email: i.email,
            server_url: i.server_url,
            device_id: i.device_id.to_string(),
        }),
        teams,
    })
}

#[tauri::command(async)]
pub fn teams_status(state: State<'_, TeamsState>) -> Result<TeamsStatus, CommandError> {
    status(&state).map_err(command_error)
}

pub(super) fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "desktop".to_string())
}

#[tauri::command(async)]
pub fn teams_init(
    state: State<'_, TeamsState>,
    server: String,
    email: String,
    device_name: Option<String>,
    passphrase: String,
    invite_token: Option<String>,
    email_code: Option<String>,
) -> std::result::Result<TeamsStatus, TeamsInitError> {
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.chars().count() < 8 {
        return Err(TeamsInitError::failed(
            "Use a passphrase of at least 8 characters.",
        ));
    }
    let name = device_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(default_device_name);
    let keys = on_init_thread(move || {
        let (store, _) = open_store()?;
        block_on(session::init(
            &store,
            &server,
            &email,
            &name,
            &passphrase,
            invite_token.as_deref(),
            email_code.as_deref(),
        ))
        .map_err(|e| match e.downcast_ref::<TeamsError>() {
            // A wrong code keeps the one in the inbox: no new mail until asked.
            Some(teams) if teams.is_invalid_email_code() => TeamsInitError::EmailCodeRejected {
                email: email.clone(),
            },
            // The server wants proof of the email: mail a code and let the
            // form ask for it (the desktop has no terminal to prompt on).
            Some(teams) if teams.is_email_verification_required() => {
                match block_on(session::request_email_code(&server, &email)) {
                    Ok(sent) => TeamsInitError::EmailCodeSent {
                        email: email.clone(),
                        expires_in_secs: sent.expires_in_secs,
                    },
                    Err(e) => TeamsInitError::from(e),
                }
            }
            _ => TeamsInitError::from(e),
        })?;
        Ok(session::unlock(&store, &passphrase)?)
    })?;
    state.set(Some(keys));
    status(&state).map_err(TeamsInitError::from)
}

/// Mail a new verification code (expiry included), e.g. when the last one
/// expired before it was typed.
#[tauri::command(async)]
pub fn teams_request_email_code(
    server: String,
    email: String,
) -> Result<EmailCodeDto, CommandError> {
    on_teams_thread(move || {
        let sent = block_on(session::request_email_code(&server, &email))?;
        Ok(EmailCodeDto {
            expires_in_secs: sent.expires_in_secs,
        })
    })
}

#[tauri::command(async)]
pub fn teams_unlock(
    state: State<'_, TeamsState>,
    passphrase: String,
) -> Result<TeamsStatus, CommandError> {
    let passphrase = Zeroizing::new(passphrase);
    let keys = on_teams_thread(move || {
        let (store, _) = open_store()?;
        session::unlock(&store, &passphrase)
    })?;
    state.set(Some(keys));
    status(&state).map_err(command_error)
}

#[tauri::command(async)]
pub fn teams_lock(state: State<'_, TeamsState>) {
    state.set(None);
}
