//! This machine's Teams identity: creating the account, unlocking the device
//! keys, email verification and revoking the device.

use super::handle::Session;
use super::local::uuid;
use crate::database::Database;
use crate::services::teams::client::{TeamsClient, TeamsError, validate_server_url};
use crate::services::teams::crypto::DeviceKeys;
use crate::services::teams::store::TeamsStore;
use anyhow::{Result, anyhow, bail};
use bssh_teams_proto::api::{CreateAccountRequest, EmailCodeRequest, EmailCodeResponse, ErrorCode};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;
use uuid::Uuid;

/// This machine's Teams account and device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub server_url: String,
    pub email: String,
    pub account_id: Uuid,
    pub device_id: Uuid,
}

pub fn load_identity(db: &Database) -> Result<Option<Identity>> {
    let row: Option<(String, String, String, String)> = db
        .conn
        .query_row(
            "SELECT server_url, email, account_id, device_id FROM teams_identity WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(server_url, email, account, device)| {
        Ok(Identity {
            server_url,
            email,
            account_id: uuid(&account)?,
            device_id: uuid(&device)?,
        })
    })
    .transpose()
}

pub(super) fn require_identity(db: &Database) -> Result<Identity> {
    load_identity(db)?.ok_or_else(|| {
        anyhow!(
            "Teams isn't set up on this machine: run `bssh teams init --server <url> --email <you>`"
        )
    })
}

/// Decrypt this machine's device keys with `passphrase`.
pub fn unlock(store: &TeamsStore, passphrase: &str) -> Result<Arc<DeviceKeys>> {
    let identity = require_identity(store.database())?;
    let sealed = store
        .sealed_device_keys(identity.device_id)?
        .ok_or_else(|| anyhow!("this device's keys are missing from the local database"))?;
    Ok(Arc::new(
        DeviceKeys::unseal(&sealed, passphrase).map_err(|_| anyhow!("wrong Teams passphrase"))?,
    ))
}

/// Create an account and this machine's device on `server_url`. The device's
/// private keys are sealed with `passphrase` and never leave the machine.
/// `invite_token` is required by servers that only accept invited accounts.
/// `email_code` is the code mailed by [`request_email_code`] when the server
/// answered `EmailVerificationRequired`; [`init_interactive`] gets it for the
/// CLI. Nothing is stored before the server accepts the account, so a failed
/// attempt leaves no identity behind and can be retried.
pub async fn init(
    store: &TeamsStore,
    server_url: &str,
    email: &str,
    device_name: &str,
    passphrase: &str,
    invite_token: Option<&str>,
    email_code: Option<&str>,
) -> Result<Identity> {
    if let Some(existing) = load_identity(store.database())? {
        bail!(
            "this machine is already set up for Teams as {} on {}",
            existing.email,
            existing.server_url
        );
    }
    let server_url = validate_server_url(server_url)?;
    let email = email.trim().to_lowercase();
    if !email.contains('@') {
        bail!("'{email}' doesn't look like an email address");
    }
    let keys = DeviceKeys::generate();
    // Account creation is unauthenticated; the signer is never used.
    let anon = TeamsClient::new(&server_url, Uuid::nil(), Arc::new(DeviceKeys::generate()))?;
    let created = anon
        .create_account(&CreateAccountRequest {
            email: email.clone(),
            device: keys.to_new_device(device_name),
            invite_token: invite_token
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            email_code: email_code
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string),
        })
        .await
        .map_err(|e| {
            if e.code() == Some(ErrorCode::Forbidden) {
                anyhow!(
                    "{server_url} only accepts invited accounts: ask your team admin for an \
                     invite, then pass its token (`--invite <TOKEN>`, or paste the invite in \
                     the desktop app)"
                )
            } else {
                anyhow::Error::from(e)
            }
        })?;
    let identity = Identity {
        server_url,
        email,
        account_id: created.account_id,
        device_id: created.device_id,
    };
    store.save_sealed_device_keys(identity.device_id, &keys.seal(passphrase)?)?;
    store.database().conn.execute(
        "INSERT INTO teams_identity (id, server_url, email, account_id, device_id, created_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            identity.server_url,
            identity.email,
            identity.account_id.to_string(),
            identity.device_id.to_string(),
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(identity)
}

/// Mail a one-time code to `email` (unauthenticated), to be passed to [`init`]
/// as `email_code`. The server answers the same whether or not a code was
/// sent, so this can't be used to learn which addresses exist.
pub async fn request_email_code(server_url: &str, email: &str) -> Result<EmailCodeResponse> {
    let server_url = validate_server_url(server_url)?;
    let anon = TeamsClient::new(&server_url, Uuid::nil(), Arc::new(DeviceKeys::generate()))?;
    Ok(anon
        .request_email_code(&EmailCodeRequest {
            email: email.trim().to_lowercase(),
        })
        .await?)
}

/// Wrong codes accepted before [`init_interactive`] gives up.
const MAX_EMAIL_CODE_TRIES: usize = 3;

/// Codes one run may mail: the first, plus one per empty line the user enters.
const MAX_EMAIL_CODES: usize = 3;

/// Reads the code that proves the account's email. The CLI asks on a terminal;
/// tests answer from a script.
pub trait EmailCodePrompt {
    /// True when there is a terminal to ask on.
    fn terminal(&self) -> bool;
    /// Show `notice` (if any) and read one line: the code, or `None` for an
    /// empty line ("mail me a new code").
    fn read_code(&mut self, notice: Option<&str>) -> Result<Option<String>>;
}

/// What `bssh teams init` was given.
#[derive(Debug, Clone, Copy)]
pub struct InitParams<'a> {
    pub server_url: &'a str,
    pub email: &'a str,
    pub device_name: &'a str,
    pub passphrase: &'a str,
    pub invite_token: Option<&'a str>,
    pub email_code: Option<&'a str>,
}

/// [`init`] for `bssh teams init`: when the server answers
/// `EmailVerificationRequired`, mail a code, ask for it on the terminal and
/// retry with it. A refused code is asked for again, up to
/// [`MAX_EMAIL_CODE_TRIES`] times; an empty line mails a new code (up to
/// [`MAX_EMAIL_CODES`] per run).
///
/// Without a terminal the code is mailed anyway and the error says to rerun
/// with `--email-code <CODE>`.
///
/// # Errors
///
/// Whatever [`init`] fails with, once the verification retries are used up.
pub async fn init_interactive(
    store: &TeamsStore,
    params: &InitParams<'_>,
    prompt: &mut dyn EmailCodePrompt,
) -> Result<Identity> {
    let InitParams {
        server_url,
        email,
        device_name,
        passphrase,
        invite_token,
        email_code,
    } = *params;
    let mut code = email_code
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    // The address the server sees, used in the notices too.
    let email = email.trim().to_lowercase();
    let mut refused = 0;
    let mut mailed = 0;
    loop {
        let err = match init(
            store,
            server_url,
            &email,
            device_name,
            passphrase,
            invite_token,
            code.as_deref(),
        )
        .await
        {
            Ok(identity) => return Ok(identity),
            Err(e) => e,
        };
        let Some(teams) = err.downcast_ref::<TeamsError>() else {
            return Err(err);
        };
        let wants_new_code = teams.is_email_verification_required();
        if !wants_new_code && !teams.is_invalid_email_code() {
            return Err(err);
        }
        // The first "send me a code" isn't a refusal; anything else (a code
        // we gave was rejected, or the server still wants one) counts.
        if !wants_new_code || code.is_some() {
            refused += 1;
            if refused >= MAX_EMAIL_CODE_TRIES {
                bail!(
                    "{server_url} refused the verification code {refused} times; wait for a \
                     newer code to arrive and rerun with `--email-code <CODE>`"
                );
            }
        }
        if !prompt.terminal() {
            let expires = mail_code(server_url, &email).await?;
            bail!(
                "{}: a new code was sent to {email} (valid {}), and this run has no terminal \
                 to type it in; rerun with `--email-code <CODE>`",
                if wants_new_code {
                    format!("{server_url} needs to verify {email}")
                } else {
                    "the verification code was rejected".to_string()
                },
                validity(expires)
            );
        }
        let mut notice = if wants_new_code && code.is_none() {
            mailed += 1;
            Some(sent_notice(&email, mail_code(server_url, &email).await?))
        } else {
            Some("That code is wrong, expired or already used.".to_string())
        };
        loop {
            match prompt.read_code(notice.as_deref())? {
                Some(typed) => {
                    code = Some(typed);
                    break;
                }
                None => {
                    // Empty line: mail a new code and ask again.
                    if mailed >= MAX_EMAIL_CODES {
                        bail!(
                            "this run already mailed {mailed} codes; wait for one to arrive \
                             and rerun with `--email-code <CODE>`"
                        );
                    }
                    mailed += 1;
                    notice = Some(sent_notice(&email, mail_code(server_url, &email).await?));
                }
            }
        }
    }
}

/// "A verification code was sent to …", rounding up so a 90-second code
/// doesn't read as valid for 1 minute less than it is.
fn sent_notice(email: &str, expires_in_secs: u64) -> String {
    format!(
        "A verification code was sent to {email} (valid {}).",
        validity(expires_in_secs)
    )
}

/// "15 minutes" / "1 minute", never "0 minutes".
fn validity(expires_in_secs: u64) -> String {
    let mins = minutes(expires_in_secs);
    if mins == 1 {
        "1 minute".to_string()
    } else {
        format!("{mins} minutes")
    }
}

pub(super) fn minutes(secs: u64) -> u64 {
    secs.div_ceil(60).max(1)
}

async fn mail_code(server_url: &str, email: &str) -> Result<u64> {
    Ok(request_email_code(server_url, email).await?.expires_in_secs)
}

// ── Team sources (the local name of a team) ─────────────────────────────────

impl<'a> Session<'a> {
    /// Revoke a device, permanently: the server kills its tokens at once and
    /// asks every team its account is in for a new key version that leaves it
    /// out. Allowed for the device's own account and for an admin of a team
    /// the device's account is in. Returns whether it was this machine's own
    /// device; if so, this machine's Teams identity is removed, since the
    /// server refuses everything from it from now on.
    pub async fn revoke_device(&self, device_id: Uuid) -> Result<bool> {
        self.client.revoke_device(device_id).await.map_err(|e| {
            if e.code() == Some(ErrorCode::NotFound) {
                // The server answers the same for a device that doesn't exist
                // and one the caller may not touch.
                anyhow!(
                    "no device {device_id} that you can revoke: it must be on your account, \
                     or belong to a member of a team you administer (see `bssh teams members`)"
                )
            } else {
                anyhow::Error::from(e)
            }
        })?;
        let mine = device_id == self.identity.device_id;
        if mine {
            self.forget_identity()?;
        }
        Ok(mine)
    }

    /// Drop this machine's Teams identity and its sealed device keys. Team
    /// connections already synced stay as ordinary local connections.
    fn forget_identity(&self) -> Result<()> {
        self.db()
            .conn
            .execute("DELETE FROM teams_identity WHERE id = 1", [])?;
        self.db().conn.execute(
            "DELETE FROM device_keys WHERE device_id = ?1",
            [self.identity.device_id.to_string()],
        )?;
        Ok(())
    }
}
