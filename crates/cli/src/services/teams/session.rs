//! The workflows behind `bssh teams`: identity setup, teams, invites, device
//! approval, sync and key rotation, built on the client, codec, store and sync
//! engine.
//!
//! Trust rule enforced here: the server never decides which devices receive a
//! team key. An admin grants a key only to a device whose fingerprint they
//! were given out of band (`grant`), and a rotation wraps the new key only to
//! devices this machine approved. A server that lists an extra device makes
//! the rotation fail instead of leaking the key.

use super::agent::{credential_path, AgentCredential};
use super::audit::{self as audit_log, ChainSummary, EventCheck};
use super::client::{validate_server_url, AuditFilter, TeamsClient, TeamsError};
use super::codec::{unwrap_all, CryptoCodec};
use super::crypto::{self, fingerprint, DeviceKeys, TeamKey};
use super::relay::{IncomingRequest, Peer, RelayEndpoint};
use super::store::{TeamLocal, TeamsStore};
use super::sync::{SyncEngine, SyncReport};
use super::{queue_connection, queue_delete, queue_edit, read_vault, VaultCodec, VaultView};
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, DiscoveredHost, Source, SourceKind};
use crate::services::inventory::sync as inventory;
use anyhow::{anyhow, bail, Context, Result};
use bssh_teams_proto::api::{
    AcceptInviteRequest, AuditKind, BillingInfo, BillingInterval, CheckoutRequest,
    ConnectionRecord, CreateAccountRequest, CreateInviteRequest, CreateTeamRequest, DeviceInfo,
    EmailCodeRequest, EmailCodeResponse, ErrorCode, Invite, Member, MemberKind, OrgInfo,
    PutKeysRequest, Role, SignedAgentPolicy, TeamInfo, VaultPayload,
};
use bssh_teams_proto::wire::{Fixed, PublicKey32};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const SCHEMA_V9: &str = "
CREATE TABLE IF NOT EXISTS teams_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    server_url TEXT NOT NULL,
    email TEXT NOT NULL,
    account_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS team_authors (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    ed25519 BLOB NOT NULL,
    PRIMARY KEY (team_id, device_id)
);
CREATE TABLE IF NOT EXISTS team_approved_devices (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    email TEXT NOT NULL,
    x25519 BLOB NOT NULL,
    ed25519 BLOB NOT NULL,
    approved_at TEXT NOT NULL,
    PRIMARY KEY (team_id, device_id)
);
CREATE TABLE IF NOT EXISTS team_audit_pending (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id TEXT NOT NULL,
    entry_id TEXT,
    kind TEXT NOT NULL,
    occurred_at TEXT NOT NULL
);
";

/// This machine's Teams account and device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub server_url: String,
    pub email: String,
    pub account_id: Uuid,
    pub device_id: Uuid,
}

/// A change to a vault, applied inside a sync.
#[derive(Debug, Clone)]
pub enum Edit {
    /// The team's display name; written once, when the team is created.
    Profile(String),
    Connection {
        entry_id: Option<Uuid>,
        record: ConnectionRecord,
    },
    Delete(Uuid),
    /// An admin-signed agent policy (bssh AI); `entry_id` re-signs an existing one.
    AgentPolicy {
        entry_id: Option<Uuid>,
        policy: SignedAgentPolicy,
    },
}

/// What one `sync_team` did.
#[derive(Debug, Default)]
pub struct TeamSync {
    pub team_id: Uuid,
    pub name: Option<String>,
    /// No team key has been granted to this device yet.
    pub waiting_for_key: bool,
    pub report: SyncReport,
    pub hosts: Option<inventory::SyncReport>,
    pub rotated_to: Option<u32>,
    pub unreadable: usize,
    /// Entry ids of the edits passed in, in order.
    pub entry_ids: Vec<Uuid>,
}

/// How to join: with a fresh invite token, or a team already accepted on the
/// server whose admin fingerprint didn't match the first time.
#[derive(Debug, Clone, Copy)]
pub enum JoinWith<'a> {
    Token(&'a str),
    Team(Uuid),
}

/// Which part of the audit log to fetch. `email` narrows client-side.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub entry_id: Option<Uuid>,
    pub email: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

/// One audit event, verified and with names resolved.
#[derive(Debug, Clone)]
pub struct AuditLine {
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub kind: AuditKind,
    /// Member email, or a placeholder for a device this machine can't name.
    pub who: String,
    pub device_name: String,
    /// The connection's name, decrypted locally; `None` if the event names no host.
    pub host: Option<String>,
    pub client_ip: Option<String>,
    pub check: EventCheck,
}

/// A team's audit log as checked by this client.
#[derive(Debug, Clone)]
pub struct TeamAudit {
    /// Newest first.
    pub lines: Vec<AuditLine>,
    pub chains: Vec<ChainSummary>,
    pub problems: Vec<String>,
    pub invalid_events: usize,
    /// The server returned fewer events than the limit: nothing was cut off.
    pub complete: bool,
}

impl TeamAudit {
    pub fn is_clean(&self) -> bool {
        self.invalid_events == 0 && self.problems.is_empty()
    }
}

/// A device as shown in a removal preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDevice {
    pub email: String,
    pub name: String,
    pub fingerprint: String,
    /// This machine approved the device (and its keys still match).
    pub approved: bool,
}

/// What removing a member would do, computed without changing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovePreview {
    pub email: String,
    pub removed_devices: Vec<PreviewDevice>,
    pub next_key_version: u32,
    /// Remaining devices that will receive the rotated key.
    pub recipients: Vec<PreviewDevice>,
    /// Remaining devices never approved here: rotation would stop.
    pub blocking: Vec<PreviewDevice>,
    pub shared_hosts: usize,
}

/// What needs a human for one team, for the desktop's attention list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamAttentionState {
    pub team_id: Uuid,
    pub role: Role,
    pub pending_devices: usize,
    pub waiting_for_key: bool,
    pub removed: bool,
}

type ApprovedDevices = HashMap<Uuid, (String, PublicKey32, PublicKey32)>;

fn preview_device(member: &Member, device: &DeviceInfo, approved: bool) -> PreviewDevice {
    PreviewDevice {
        email: member.email.clone(),
        name: device.name.clone(),
        fingerprint: fingerprint(&device.ed25519_public),
        approved,
    }
}

/// Who would get the rotated key if `email` were removed. Mirrors the checks
/// in `Session::rotate`: a device is a recipient only if this machine
/// approved it and its keys still match what was approved.
pub fn plan_removal(
    members: &[Member],
    email: &str,
    approved: &ApprovedDevices,
    current_key_version: u32,
    shared_hosts: usize,
) -> Result<RemovePreview> {
    let email = email.trim().to_lowercase();
    let removed = members
        .iter()
        .find(|m| m.email == email)
        .ok_or_else(|| anyhow!("{email} is not a member of this team"))?;
    let removed_devices = removed
        .devices
        .iter()
        .filter(|d| d.revoked_at.is_none())
        .map(|d| preview_device(removed, d, approved.contains_key(&d.device_id)))
        .collect();
    let mut recipients = Vec::new();
    let mut blocking = Vec::new();
    for member in members.iter().filter(|m| m.member_id != removed.member_id) {
        for device in member.devices.iter().filter(|d| d.revoked_at.is_none()) {
            let ok = matches!(
                approved.get(&device.device_id),
                Some((_, x, e)) if *x == device.x25519_public && *e == device.ed25519_public
            );
            let line = preview_device(member, device, ok);
            if ok {
                recipients.push(line)
            } else {
                blocking.push(line)
            }
        }
    }
    Ok(RemovePreview {
        email,
        removed_devices,
        next_key_version: current_key_version + 1,
        recipients,
        blocking,
        shared_hosts,
    })
}

/// A device waiting for a team key, for `bssh teams grant` to show.
#[derive(Debug, Clone)]
pub struct PendingDevice {
    pub email: String,
    pub device: DeviceInfo,
    pub fingerprint: String,
}

fn uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| anyhow!("corrupt id {s:?} in local teams tables: {e}"))
}

fn key32(b: Vec<u8>) -> Result<PublicKey32> {
    let a: [u8; 32] = b
        .try_into()
        .map_err(|_| anyhow!("corrupt key in local teams tables"))?;
    Ok(Fixed(a))
}

pub(crate) fn kind_str(kind: AuditKind) -> &'static str {
    kind.as_str()
}

pub(crate) fn kind_from_str(s: &str) -> Option<AuditKind> {
    AuditKind::parse(s)
}

// ── Identity ────────────────────────────────────────────────────────────────

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

fn require_identity(db: &Database) -> Result<Identity> {
    load_identity(db)?.ok_or_else(|| {
        anyhow!("Teams isn't set up on this machine: run `bssh teams init --server <url> --email <you>`")
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

/// [`init`] for `bssh teams init`: when the server answers
/// `EmailVerificationRequired`, mail a code, ask for it on the terminal and
/// retry with it. A refused code is asked for again, up to
/// [`MAX_EMAIL_CODE_TRIES`] times; an empty line mails a new code (up to
/// [`MAX_EMAIL_CODES`] per run).
///
/// Without a terminal the code is mailed anyway and the error says to rerun
/// with `--email-code <CODE>`.
#[allow(clippy::too_many_arguments)]
pub async fn init_interactive(
    store: &TeamsStore,
    server_url: &str,
    email: &str,
    device_name: &str,
    passphrase: &str,
    invite_token: Option<&str>,
    email_code: Option<&str>,
    prompt: &mut dyn EmailCodePrompt,
) -> Result<Identity> {
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

fn minutes(secs: u64) -> u64 {
    secs.div_ceil(60).max(1)
}

async fn mail_code(server_url: &str, email: &str) -> Result<u64> {
    Ok(request_email_code(server_url, email).await?.expires_in_secs)
}

// ── Team sources (the local name of a team) ─────────────────────────────────

fn team_id_of(source: &Source) -> Option<Uuid> {
    if source.kind != SourceKind::Team {
        return None;
    }
    source
        .config
        .get("team_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Every team this machine knows, as `(team_id, source)`.
pub fn local_teams(db: &Database) -> Result<Vec<(Uuid, Source)>> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter_map(|s| team_id_of(&s.source).map(|id| (id, s.source)))
        .collect())
}

/// Every team this machine knows, as `(team_id, local name)`. Organizations
/// are named by their teams in `<org>` arguments.
pub fn local_team_names(db: &Database) -> Result<Vec<(Uuid, String)>> {
    Ok(local_teams(db)?
        .into_iter()
        .map(|(team_id, source)| (team_id, source.name))
        .collect())
}

/// Find a team by its local name, or by a prefix of its id.
pub fn resolve_team(db: &Database, name_or_id: &str) -> Result<(Uuid, Source)> {
    let teams = local_teams(db)?;
    if let Some(t) = teams.iter().find(|(_, s)| s.name == name_or_id) {
        return Ok(t.clone());
    }
    let by_id: Vec<_> = teams
        .iter()
        .filter(|(id, _)| id.to_string().starts_with(&name_or_id.to_lowercase()))
        .collect();
    match by_id.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("no team named '{name_or_id}' (see `bssh teams list`)"),
        _ => bail!("'{name_or_id}' matches several teams; use more of the id"),
    }
}

pub(crate) fn team_of_connection(
    db: &Database,
    connection: &Connection,
) -> Result<Option<(Uuid, Uuid)>> {
    let Some(source_id) = connection.source_id else {
        return Ok(None);
    };
    let Some(source) = db.get_source(&source_id.to_string())? else {
        return Ok(None);
    };
    let (Some(team_id), Some(entry)) = (team_id_of(&source), connection.external_id.as_deref())
    else {
        return Ok(None);
    };
    Ok(Some((team_id, uuid(entry)?)))
}

/// Queue a "session started" audit event if `connection` comes from a team
/// vault. Needs no passphrase: the event is signed and uploaded by the next
/// `bssh teams sync`, with the time it actually happened.
pub fn record_session_start(db: &Database, connection: &Connection) -> Result<()> {
    if let Some((team_id, entry_id)) = team_of_connection(db, connection)? {
        db.conn.execute(
            "INSERT INTO team_audit_pending (team_id, entry_id, kind, occurred_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                team_id.to_string(),
                entry_id.to_string(),
                kind_str(AuditKind::SessionStart),
                Utc::now().to_rfc3339()
            ],
        )?;
    }
    Ok(())
}

// ── Session ─────────────────────────────────────────────────────────────────

/// An unlocked identity: device keys in memory and an authenticated client.
pub struct Session<'a> {
    store: &'a TeamsStore,
    config: &'a AppConfig,
    pub identity: Identity,
    keys: Arc<DeviceKeys>,
    client: TeamsClient,
}

impl<'a> Session<'a> {
    /// Unlock this machine's device keys with `passphrase`.
    pub fn open(store: &'a TeamsStore, config: &'a AppConfig, passphrase: &str) -> Result<Self> {
        let keys = unlock(store, passphrase)?;
        Self::with_keys(store, config, keys)
    }

    /// A session over keys unlocked earlier with [`unlock`], so a long-lived
    /// process (the desktop app) pays for the passphrase KDF only once.
    pub fn with_keys(
        store: &'a TeamsStore,
        config: &'a AppConfig,
        keys: Arc<DeviceKeys>,
    ) -> Result<Self> {
        let identity = require_identity(store.database())?;
        let client = TeamsClient::new(&identity.server_url, identity.device_id, keys.clone())?;
        Ok(Self {
            store,
            config,
            identity,
            keys,
            client,
        })
    }

    fn db(&self) -> &Database {
        self.store.database()
    }

    /// This device's fingerprint, to send to an admin for `bssh teams grant`.
    pub fn my_fingerprint(&self) -> String {
        fingerprint(&self.keys.ed25519_public())
    }

    fn my_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: self.identity.device_id,
            account_id: self.identity.account_id,
            name: String::new(),
            x25519_public: self.keys.x25519_public(),
            ed25519_public: self.keys.ed25519_public(),
            created_at: Utc::now(),
            revoked_at: None,
        }
    }

    fn local_team(&self, team_id: Uuid) -> Result<TeamLocal> {
        self.store
            .get_team(team_id)?
            .ok_or_else(|| anyhow!("team {team_id} is not set up on this machine"))
    }

    async fn team_info(&self, team_id: Uuid) -> Result<TeamInfo> {
        match self.client.get_team(team_id).await {
            Ok(info) => Ok(info),
            Err(e) if e.code() == Some(ErrorCode::Forbidden) => {
                bail!("you're no longer a member of this team")
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn require_admin(&self, team_id: Uuid) -> Result<TeamInfo> {
        let info = self.team_info(team_id).await?;
        if info.my_role != Role::Admin {
            bail!("only a team admin can do that");
        }
        Ok(info)
    }

    // -- local trust state ---------------------------------------------------

    fn remember_authors(&self, team_id: Uuid, members: &[Member]) -> Result<()> {
        for device in members.iter().flat_map(|m| &m.devices) {
            self.db().conn.execute(
                "INSERT OR IGNORE INTO team_authors (team_id, device_id, ed25519)
                 VALUES (?1, ?2, ?3)",
                params![
                    team_id.to_string(),
                    device.device_id.to_string(),
                    device.ed25519_public.as_bytes().as_slice()
                ],
            )?;
        }
        Ok(())
    }

    /// Every device that ever wrote to the team as far as this machine knows,
    /// including removed members, so their old entries stay verifiable.
    fn authors(&self, team_id: Uuid) -> Result<HashMap<Uuid, PublicKey32>> {
        let mut stmt = self
            .db()
            .conn
            .prepare("SELECT device_id, ed25519 FROM team_authors WHERE team_id = ?1")?;
        let rows = stmt.query_map([team_id.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (device, key) = row?;
            out.insert(uuid(&device)?, key32(key)?);
        }
        Ok(out)
    }

    fn approve_device(&self, team_id: Uuid, device: &DeviceInfo, email: &str) -> Result<()> {
        self.db().conn.execute(
            "INSERT OR REPLACE INTO team_approved_devices
                 (team_id, device_id, email, x25519, ed25519, approved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                team_id.to_string(),
                device.device_id.to_string(),
                email,
                device.x25519_public.as_bytes().as_slice(),
                device.ed25519_public.as_bytes().as_slice(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Devices this admin approved, keyed by id: `(email, x25519, ed25519)`.
    fn approved_devices(
        &self,
        team_id: Uuid,
    ) -> Result<HashMap<Uuid, (String, PublicKey32, PublicKey32)>> {
        let mut stmt = self.db().conn.prepare(
            "SELECT device_id, email, x25519, ed25519 FROM team_approved_devices WHERE team_id = ?1",
        )?;
        let rows = stmt.query_map([team_id.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (device, email, x, e) = row?;
            out.insert(uuid(&device)?, (email, key32(x)?, key32(e)?));
        }
        Ok(out)
    }

    // -- keys and codec ------------------------------------------------------

    /// Fetch, check and store this device's wrapped keys, then unwrap all of
    /// them against the pinned trust root. Empty if none were granted yet.
    async fn team_keys(&self, team_id: Uuid, current: u32) -> Result<Vec<TeamKey>> {
        let local = self.local_team(team_id)?;
        let incoming = self.client.get_keys(team_id).await?;
        self.store.accept_wrapped_keys(
            team_id,
            self.identity.device_id,
            &incoming,
            Some(current),
        )?;
        let wrapped = self.store.wrapped_keys(team_id)?;
        if wrapped.is_empty() {
            return Ok(Vec::new());
        }
        unwrap_all(
            team_id,
            self.identity.device_id,
            &wrapped,
            &self.keys,
            &local.trust_root_ed25519,
        )
    }

    async fn codec(&self, team_id: Uuid, current: u32) -> Result<Option<CryptoCodec>> {
        let keys = self.team_keys(team_id, current).await?;
        if keys.is_empty() {
            return Ok(None);
        }
        Ok(Some(CryptoCodec::new(
            team_id,
            self.identity.device_id,
            self.keys.clone(),
            keys,
            self.authors(team_id)?,
        )?))
    }

    async fn rotated_codec(&self, team_id: Uuid, version: u32) -> Result<CryptoCodec> {
        self.codec(team_id, version)
            .await?
            .ok_or_else(|| anyhow!("rotated team key is not available after posting it"))
    }

    // -- workflows -----------------------------------------------------------

    /// Create a team named `name` with this device as its admin and trust root.
    pub async fn create_team(&self, name: &str) -> Result<TeamSync> {
        self.create_team_in(name, None).await
    }

    /// Create a team in `org_id` (the caller must own it). With `None` the
    /// server puts it in the oldest organization the caller owns, or in a new
    /// one.
    pub async fn create_team_in(&self, name: &str, org_id: Option<Uuid>) -> Result<TeamSync> {
        if self.db().get_source(name)?.is_some() {
            bail!("a source named '{name}' already exists; pick another team name");
        }
        let team_id = Uuid::new_v4();
        let me = self.my_device();
        let tk = TeamKey::generate(1);
        let initial_key =
            crypto::wrap_team_key(team_id, &tk, &me, self.identity.device_id, &self.keys)?;
        self.client
            .create_team(&CreateTeamRequest {
                initial_key,
                org_id,
            })
            .await?;
        self.store.upsert_team(&TeamLocal {
            team_id,
            server_url: self.identity.server_url.clone(),
            account_id: self.identity.account_id,
            device_id: self.identity.device_id,
            trust_root_ed25519: self.keys.ed25519_public(),
            last_pulled_revision: 0,
            created_at: Utc::now(),
        })?;
        self.approve_device(team_id, &me, &self.identity.email)?;
        self.db().add_source(&Source::new(
            name.to_string(),
            SourceKind::Team,
            serde_json::json!({ "team_id": team_id.to_string() }),
        ))?;
        self.sync_team(team_id, vec![Edit::Profile(name.to_string())])
            .await
    }

    /// Invite `email`. Returns the invite (with its one-time token) and this
    /// admin device's fingerprint, which the invitee must pin.
    pub async fn invite(&self, team_id: Uuid, email: &str, role: Role) -> Result<(Invite, String)> {
        self.require_admin(team_id).await?;
        let invite = self
            .client
            .create_invite(
                team_id,
                &CreateInviteRequest {
                    email: email.trim().to_lowercase(),
                    role,
                },
            )
            .await?;
        Ok((invite, self.my_fingerprint()))
    }

    /// Accept an invite, pinning the admin device whose fingerprint the admin
    /// sent. Returns the team id and the local name it was given.
    ///
    /// The members list (and so the fingerprint check) is only visible after
    /// accepting, which uses up the single-use token. If the check fails, the
    /// error names the team id so the join can be finished with
    /// [`JoinWith::Team`] once the right fingerprint is known.
    pub async fn join(
        &self,
        with: JoinWith<'_>,
        admin_fingerprint: &str,
    ) -> Result<(Uuid, String)> {
        let team_id = match with {
            JoinWith::Token(token) => {
                self.client
                    .accept_invite(&AcceptInviteRequest {
                        token: token.trim().to_string(),
                    })
                    .await?
                    .team_id
            }
            JoinWith::Team(team_id) => self.team_info(team_id).await?.team_id,
        };
        let members = self.client.list_members(team_id).await?;
        let admin = members
            .iter()
            .filter(|m| m.role == Role::Admin)
            .flat_map(|m| &m.devices)
            .find(|d| fingerprint(&d.ed25519_public) == admin_fingerprint.trim())
            .ok_or_else(|| {
                anyhow!(
                    "no admin device of team {team_id} has fingerprint {admin_fingerprint}. \
                     Nothing was trusted. Check the fingerprint with your admin (a mismatch can \
                     also mean the server is not the one you think), then finish with \
                     `bssh teams join --team {team_id} --fingerprint <FINGERPRINT>`"
                )
            })?;
        self.store.upsert_team(&TeamLocal {
            team_id,
            server_url: self.identity.server_url.clone(),
            account_id: self.identity.account_id,
            device_id: self.identity.device_id,
            trust_root_ed25519: admin.ed25519_public,
            last_pulled_revision: 0,
            created_at: Utc::now(),
        })?;
        self.remember_authors(team_id, &members)?;
        // Renamed to the team's real name on the first sync that can read it.
        let name = format!("team-{}", &team_id.simple().to_string()[..8]);
        if self.db().get_source(&name)?.is_none() {
            self.db().add_source(&Source::new(
                name.clone(),
                SourceKind::Team,
                serde_json::json!({ "team_id": team_id.to_string() }),
            ))?;
        }
        Ok((team_id, name))
    }

    /// Devices waiting for the team key, with their owners and fingerprints.
    pub async fn pending(&self, team_id: Uuid) -> Result<Vec<PendingDevice>> {
        self.require_admin(team_id).await?;
        let pending = self.client.pending_devices(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        let email_of = |device: Uuid| {
            members
                .iter()
                .find(|m| m.devices.iter().any(|d| d.device_id == device))
                .map(|m| m.email.clone())
                .unwrap_or_default()
        };
        Ok(pending
            .devices
            .into_iter()
            .map(|d| PendingDevice {
                email: email_of(d.device_id),
                fingerprint: fingerprint(&d.ed25519_public),
                device: d,
            })
            .collect())
    }

    /// Give the current team key to the pending device with this fingerprint,
    /// and remember it as approved for future rotations.
    pub async fn grant(&self, team_id: Uuid, device_fingerprint: &str) -> Result<PendingDevice> {
        let info = self.require_admin(team_id).await?;
        let device = self
            .pending(team_id)
            .await?
            .into_iter()
            .find(|p| p.fingerprint == device_fingerprint.trim())
            .ok_or_else(|| {
                anyhow!("no device waiting for this team has fingerprint {device_fingerprint}")
            })?;
        let keys = self.team_keys(team_id, info.current_key_version).await?;
        let current = keys
            .iter()
            .find(|k| k.key_version() == info.current_key_version)
            .ok_or_else(|| anyhow!("this device doesn't hold the current team key"))?;
        let wrapped = crypto::wrap_team_key(
            team_id,
            current,
            &device.device,
            self.identity.device_id,
            &self.keys,
        )?;
        self.client
            .put_keys(
                team_id,
                &PutKeysRequest {
                    keys: vec![wrapped],
                },
            )
            .await?;
        self.approve_device(team_id, &device.device, &device.email)?;
        Ok(device)
    }

    /// Plan and seats of a team (any member).
    pub async fn billing(&self, team_id: Uuid) -> Result<BillingInfo> {
        self.team_info(team_id).await?;
        Ok(self.client.billing(team_id).await?)
    }

    /// A Stripe Checkout page to subscribe the team for `seats` seats (admin).
    pub async fn checkout_url(
        &self,
        team_id: Uuid,
        seats: u32,
        interval: BillingInterval,
    ) -> Result<String> {
        // No client-side role check: billing belongs to the team's organization,
        // and only the server knows who owns it (a team admin may not be an owner).
        let redirect = self
            .client
            .billing_checkout(team_id, &CheckoutRequest { seats, interval })
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// The Stripe Customer Portal of the team's organization: card, invoices,
    /// seats, cancellation (organization owner).
    pub async fn portal_url(&self, team_id: Uuid) -> Result<String> {
        let redirect = self
            .client
            .billing_portal(team_id)
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    pub async fn members(&self, team_id: Uuid) -> Result<Vec<Member>> {
        self.team_info(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;
        Ok(members)
    }

    // -- organizations -------------------------------------------------------

    /// Organizations this account owns or has a team in.
    pub async fn list_orgs(&self) -> Result<Vec<OrgInfo>> {
        Ok(self.client.list_orgs().await?)
    }

    /// Create an empty organization owned by this account. The free plan
    /// covers one per account: a second one needs a subscription.
    pub async fn create_org(&self) -> Result<OrgInfo> {
        match self.client.create_org().await {
            Ok(org) => Ok(org),
            Err(e) if e.code() == Some(ErrorCode::PaymentRequired) => Err(anyhow!(e).context(
                "one organization is free per account: subscribe to your existing one \
                 (`bssh org billing <org> --checkout`) to create another, or create teams in it",
            )),
            Err(e) => Err(org_message(e)),
        }
    }

    pub async fn org_info(&self, org_id: Uuid) -> Result<OrgInfo> {
        Ok(self.client.get_org(org_id).await?)
    }

    /// Add an owner (the server requires them to be in one of its teams).
    pub async fn add_org_owner(&self, org_id: Uuid, email: &str) -> Result<OrgInfo> {
        let email = email.trim().to_lowercase();
        self.client
            .add_org_owner(org_id, &email)
            .await
            .map_err(org_message)
    }

    /// Remove an owner, resolving the email to an account id from the
    /// organization's owner list (the server takes an account id).
    pub async fn remove_org_owner(&self, org_id: Uuid, email: &str) -> Result<OrgInfo> {
        let email = email.trim().to_lowercase();
        let org = self.client.get_org(org_id).await?;
        let account_id = org
            .owners
            .iter()
            .find(|o| o.email == email)
            .map(|o| o.account_id)
            .ok_or_else(|| anyhow!("{email} is not an owner of this organization"))?;
        self.client
            .remove_org_owner(org_id, account_id)
            .await
            .map_err(org_message)?;
        // As it is now: callers label the organization by its remaining owners.
        Ok(self.client.get_org(org_id).await?)
    }

    /// Plan and seats of an organization (any member).
    pub async fn org_billing(&self, org_id: Uuid) -> Result<BillingInfo> {
        Ok(self.client.org_billing(org_id).await?)
    }

    /// A provider checkout page for the organization's subscription (owner).
    pub async fn org_checkout_url(
        &self,
        org_id: Uuid,
        seats: u32,
        interval: BillingInterval,
    ) -> Result<String> {
        let redirect = self
            .client
            .org_billing_checkout(org_id, &CheckoutRequest { seats, interval })
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// The organization's customer portal: card, invoices, seats, cancellation (owner).
    pub async fn org_portal_url(&self, org_id: Uuid) -> Result<String> {
        let redirect = self
            .client
            .org_billing_portal(org_id)
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// Pending invites, which hold seats until accepted or expired. Only
    /// admins can see them: `None` for a member, or on a server too old to
    /// list them (no such route: a bare 404 or 405).
    pub async fn invites(&self, team_id: Uuid) -> Result<Option<Vec<Invite>>> {
        if self.team_info(team_id).await?.my_role != Role::Admin {
            return Ok(None);
        }
        match self.client.list_invites(team_id).await {
            Ok(invites) => Ok(Some(invites)),
            Err(TeamsError::Http {
                status: 404 | 405, ..
            }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Revoke the pending invite for `email`: its token stops working and its
    /// seat is free again.
    pub async fn uninvite(&self, team_id: Uuid, email: &str) -> Result<Invite> {
        self.require_admin(team_id).await?;
        let email = email.trim().to_lowercase();
        let invite = self
            .invites(team_id)
            .await?
            .ok_or_else(|| {
                anyhow!("this Teams server can't revoke invites yet: it needs an update")
            })?
            .into_iter()
            .find(|i| i.email == email)
            .ok_or_else(|| anyhow!("no pending invite for {email}"))?;
        self.client.revoke_invite(team_id, invite.invite_id).await?;
        Ok(invite)
    }

    /// What needs a human in this team. A removed account is reported, not
    /// raised as an error.
    pub async fn attention(&self, team_id: Uuid) -> Result<TeamAttentionState> {
        let info = match self.client.get_team(team_id).await {
            Ok(info) => info,
            Err(e) if e.code() == Some(ErrorCode::Forbidden) => {
                return Ok(TeamAttentionState {
                    team_id,
                    role: Role::Member,
                    pending_devices: 0,
                    waiting_for_key: false,
                    removed: true,
                })
            }
            Err(e) => return Err(e.into()),
        };
        let waiting_for_key = self
            .team_keys(team_id, info.current_key_version)
            .await?
            .is_empty();
        let pending_devices = if info.my_role == Role::Admin {
            self.client.pending_devices(team_id).await?.devices.len()
        } else {
            0
        };
        Ok(TeamAttentionState {
            team_id,
            role: info.my_role,
            pending_devices,
            waiting_for_key,
            removed: false,
        })
    }

    /// What `remove` would do, without changing anything.
    pub async fn remove_preview(&self, team_id: Uuid, email: &str) -> Result<RemovePreview> {
        let info = self.require_admin(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        let approved = self.approved_devices(team_id)?;
        let shared_hosts = self.live_hosts(team_id)?;
        plan_removal(
            &members,
            email,
            &approved,
            info.current_key_version,
            shared_hosts,
        )
    }

    fn live_hosts(&self, team_id: Uuid) -> Result<usize> {
        Ok(self
            .db()
            .list_sources()?
            .into_iter()
            .find(|s| team_id_of(&s.source) == Some(team_id))
            .map_or(0, |s| s.hosts.saturating_sub(s.stale)))
    }

    /// Remove the member with `email`, then rotate the team key so they can't
    /// read anything written from now on.
    pub async fn remove(&self, team_id: Uuid, email: &str) -> Result<TeamSync> {
        self.require_admin(team_id).await?;
        let email = email.trim().to_lowercase();
        let members = self.client.list_members(team_id).await?;
        let member = members
            .iter()
            .find(|m| m.email == email)
            .ok_or_else(|| anyhow!("{email} is not a member of this team"))?;
        if member.account_id == self.identity.account_id {
            bail!("you can't remove yourself");
        }
        self.client.remove_member(team_id, member.member_id).await?;
        for device in &member.devices {
            self.db().conn.execute(
                "DELETE FROM team_approved_devices WHERE team_id = ?1 AND device_id = ?2",
                params![team_id.to_string(), device.device_id.to_string()],
            )?;
        }
        // The member is gone server-side whatever happens next: say so, so a
        // failed rotation isn't read as a failed removal.
        self.sync_team(team_id, Vec::new()).await.map_err(|e| {
            anyhow!(
                "{email} is removed from the team, but {e:#}. Until the key is rotated, \
                 nobody can push; `bssh teams sync` rotates it once that is fixed"
            )
        })
    }

    /// New key version for the remaining, approved devices. Queued edits
    /// sealed under the old key are re-sealed under the new one.
    async fn rotate(&self, team_id: Uuid, info: &TeamInfo, old: &CryptoCodec) -> Result<u32> {
        let members = self.client.list_members(team_id).await?;
        let approved = self.approved_devices(team_id)?;
        let mut recipients = Vec::new();
        let mut unapproved = Vec::new();
        for member in &members {
            for device in member.devices.iter().filter(|d| d.revoked_at.is_none()) {
                match approved.get(&device.device_id) {
                    Some((_, x, e))
                        if *x == device.x25519_public && *e == device.ed25519_public =>
                    {
                        recipients.push(device.clone())
                    }
                    _ => unapproved.push(format!(
                        "{} ({})",
                        member.email,
                        fingerprint(&device.ed25519_public)
                    )),
                }
            }
        }
        if !unapproved.is_empty() {
            bail!(
                "can't rotate the team key: these devices were never approved on this machine: {}. \
                 Approve each with `bssh teams grant` once you've checked its fingerprint, \
                 or remove the member",
                unapproved.join(", ")
            );
        }
        let version = info.current_key_version + 1;
        let tk = TeamKey::generate(version);
        let keys = recipients
            .iter()
            .map(|d| crypto::wrap_team_key(team_id, &tk, d, self.identity.device_id, &self.keys))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.client
            .put_keys(team_id, &PutKeysRequest { keys })
            .await
            .context("posting the rotated team key")?;

        let new = self.rotated_codec(team_id, version).await?;
        for row in self.store.outbox(team_id)? {
            if row.entry.key_version < version {
                let payload = old.open(&row.entry)?;
                queue_edit(self.store, &new, team_id, row.entry.entry_id, &payload)?;
            }
        }
        Ok(version)
    }

    fn sign_pending_audit(&self, team_id: Uuid, codec: &CryptoCodec) -> Result<()> {
        let rows: Vec<(i64, Option<String>, String, String)> = {
            let mut stmt = self.db().conn.prepare(
                "SELECT id, entry_id, kind, occurred_at FROM team_audit_pending
                 WHERE team_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([team_id.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, entry, kind, at) in rows {
            if let Some(kind) = kind_from_str(&kind) {
                let entry = entry.as_deref().map(uuid).transpose()?;
                let at = DateTime::parse_from_rfc3339(&at)
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                self.store
                    .append_audit_event_at(team_id, codec, kind, entry, at)?;
            }
            self.db()
                .conn
                .execute("DELETE FROM team_audit_pending WHERE id = ?1", [id])?;
        }
        // bssh AI events queued by `ai::teams::forward_event`.
        crate::services::ai::teams::flush_pending(self.store, codec, team_id)?;
        Ok(())
    }

    /// Pull, apply `edits`, push, upload audit events, rotate the key if an
    /// admin must, and mirror the vault into the local connection list.
    pub async fn sync_team(&self, team_id: Uuid, edits: Vec<Edit>) -> Result<TeamSync> {
        let mut out = TeamSync {
            team_id,
            ..Default::default()
        };
        let mut info = self.team_info(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;

        let Some(mut codec) = self.codec(team_id, info.current_key_version).await? else {
            if !edits.is_empty() {
                bail!("this device has no team key yet; ask an admin to run `bssh teams grant`");
            }
            out.waiting_for_key = true;
            return Ok(out);
        };

        let is_admin = info.my_role == Role::Admin;
        if is_admin && info.rotation_required {
            let version = self.rotate(team_id, &info, &codec).await?;
            codec = self.rotated_codec(team_id, version).await?;
            out.rotated_to = Some(version);
        }

        for edit in edits {
            let id = match edit {
                Edit::Profile(name) => {
                    let id = Uuid::new_v4();
                    queue_edit(
                        self.store,
                        &codec,
                        team_id,
                        id,
                        &VaultPayload::TeamProfile { name },
                    )?;
                    id
                }
                Edit::Connection { entry_id, record } => {
                    queue_connection(self.store, &codec, team_id, entry_id, record)?
                }
                Edit::Delete(id) => {
                    queue_delete(self.store, &codec, team_id, id)?;
                    id
                }
                Edit::AgentPolicy { entry_id, policy } => {
                    let id = entry_id.unwrap_or_else(Uuid::new_v4);
                    queue_edit(
                        self.store,
                        &codec,
                        team_id,
                        id,
                        &VaultPayload::AgentPolicy(policy),
                    )?;
                    id
                }
            };
            out.entry_ids.push(id);
        }

        self.sign_pending_audit(team_id, &codec)?;
        out.report = SyncEngine::new(self.store, &self.client, &codec, team_id)
            .sync()
            .await;

        if out.report.rotation_required && is_admin && out.rotated_to.is_none() {
            info = self.team_info(team_id).await?;
            let version = self.rotate(team_id, &info, &codec).await?;
            codec = self.rotated_codec(team_id, version).await?;
            out.rotated_to = Some(version);
            out.report = SyncEngine::new(self.store, &self.client, &codec, team_id)
                .sync()
                .await;
        }

        let view = read_vault(self.store, &codec, team_id)?;
        out.unreadable = view.unreadable.len();
        out.name = view.team_name.clone();
        out.hosts = Some(self.materialize(team_id, &view)?);
        Ok(out)
    }

    /// Mirror the decrypted vault into the connection list through the
    /// inventory machinery: hosts are keyed by entry id, removed ones go stale.
    fn materialize(&self, team_id: Uuid, view: &VaultView) -> Result<inventory::SyncReport> {
        materialize_into(self.db(), self.config, team_id, view)
    }

    /// Share a local connection with the team (or update it, if it already
    /// came from this team).
    pub async fn share(&self, team_id: Uuid, connection: &Connection) -> Result<(Uuid, TeamSync)> {
        let entry_id = match team_of_connection(self.db(), connection)? {
            Some((t, entry)) if t == team_id => Some(entry),
            Some(_) => bail!("'{}' belongs to another team", connection.name),
            None => None,
        };
        let record = ConnectionRecord {
            name: connection.name.clone(),
            host: connection.host.clone(),
            user: connection.user.clone(),
            port: connection.port,
            bastion: connection.bastion.clone(),
            bastion_user: connection.bastion_user.clone(),
            use_kerberos: connection.use_kerberos,
            tags: connection.tags.clone(),
            notes: None,
        };
        let sync = self
            .sync_team(team_id, vec![Edit::Connection { entry_id, record }])
            .await?;
        let id = sync.entry_ids.first().copied().unwrap_or_default();
        Ok((id, sync))
    }

    /// Delete a team connection from the vault for everyone.
    pub async fn unshare(&self, connection: &Connection) -> Result<TeamSync> {
        let (team_id, entry_id) = team_of_connection(self.db(), connection)?
            .ok_or_else(|| anyhow!("'{}' is not a team connection", connection.name))?;
        self.sync_team(team_id, vec![Edit::Delete(entry_id)]).await
    }

    /// The entry id of a team connection by its local name or alias.
    pub fn entry_for_connection(&self, team_id: Uuid, name: &str) -> Result<Uuid> {
        let conn = self
            .db()
            .get_connection_or_alias(name)?
            .ok_or_else(|| anyhow!("no connection named '{name}'"))?;
        match team_of_connection(self.db(), &conn)? {
            Some((t, entry)) if t == team_id => Ok(entry),
            _ => bail!("'{name}' is not a connection of this team"),
        }
    }

    /// Download the audit log (admins only) and verify it here: signatures
    /// against device keys this machine knows, and each device's hash chain.
    pub async fn audit(&self, team_id: Uuid, query: AuditQuery) -> Result<TeamAudit> {
        let info = self.require_admin(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;

        let limit = query.limit.unwrap_or(1000).clamp(1, 1000);
        let records = self
            .client
            .list_audit(
                team_id,
                &AuditFilter {
                    entry_id: query.entry_id,
                    since: query.since,
                    limit: Some(limit),
                    ..Default::default()
                },
            )
            .await?;
        let complete = records.len() < limit as usize;
        let filtered = query.entry_id.is_some() || query.since.is_some() || query.email.is_some();
        let verified = audit_log::verify(team_id, records, &self.authors(team_id)?, filtered);

        // Names: current members first, then devices this admin approved
        // (which covers members removed since).
        let mut names: HashMap<Uuid, (String, String)> = HashMap::new();
        for (device, (email, _, _)) in self.approved_devices(team_id)? {
            names.insert(device, (email, String::new()));
        }
        for m in &members {
            for d in &m.devices {
                names.insert(d.device_id, (m.email.clone(), d.name.clone()));
            }
        }
        let hosts: HashMap<Uuid, String> =
            match self.codec(team_id, info.current_key_version).await? {
                Some(codec) => read_vault(self.store, &codec, team_id)?
                    .connections
                    .into_iter()
                    .map(|c| (c.entry_id, c.record.name))
                    .collect(),
                None => HashMap::new(),
            };

        let email_filter = query.email.map(|e| e.trim().to_lowercase());
        let lines = verified
            .records
            .iter()
            .filter_map(|r| {
                let e = &r.record.event;
                let (who, device_name) = names
                    .get(&e.device_id)
                    .cloned()
                    .unwrap_or_else(|| (format!("unknown device {}", e.device_id), String::new()));
                if email_filter.as_deref().is_some_and(|f| f != who) {
                    return None;
                }
                Some(AuditLine {
                    occurred_at: e.occurred_at,
                    received_at: r.record.received_at,
                    kind: e.kind,
                    who,
                    device_name,
                    host: e.entry_id.map(|id| {
                        hosts
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| "(deleted host)".to_string())
                    }),
                    client_ip: r.record.client_ip.clone(),
                    check: r.check,
                })
            })
            .collect();

        Ok(TeamAudit {
            lines,
            invalid_events: verified.invalid_events(),
            problems: verified.problems.iter().map(|p| p.to_string()).collect(),
            chains: verified.chains,
            complete,
        })
    }
}

/// Mirror a decrypted vault into `db` through the inventory machinery: hosts are
/// keyed by entry id, removed ones go stale. The team needs a local source.
pub(crate) fn materialize_into(
    db: &Database,
    config: &AppConfig,
    team_id: Uuid,
    view: &VaultView,
) -> Result<inventory::SyncReport> {
    let (_, mut source) = local_teams(db)?
        .into_iter()
        .find(|(id, _)| *id == team_id)
        .ok_or_else(|| anyhow!("team {team_id} has no local source"))?;

    if let Some(name) = &view.team_name {
        if source.name != *name
            && source.name.starts_with("team-")
            && db.get_source(name)?.is_none()
        {
            db.conn.execute(
                "UPDATE sources SET name = ?1 WHERE id = ?2",
                params![name, source.id.to_string()],
            )?;
            source.name = name.clone();
        }
    }

    let discovered: Vec<DiscoveredHost> = view
        .connections
        .iter()
        .map(|c| DiscoveredHost {
            external_id: c.entry_id.to_string(),
            name: c.record.name.clone(),
            host: c.record.host.clone(),
            user: Some(c.record.user.clone()),
            port: Some(c.record.port),
            key_path: None,
            tags: c.record.tags.clone(),
            bastion: c.record.bastion.clone(),
            bastion_user: c.record.bastion_user.clone(),
            use_kerberos: c.record.use_kerberos,
        })
        .collect();
    let existing = db.connections_for_source(source.id)?;
    let taken: HashSet<String> = db
        .list_connections(None, false)?
        .into_iter()
        .filter(|c| c.source_id != Some(source.id))
        .map(|c| c.name)
        .collect();
    let plan = inventory::plan(&discovered, &existing, &taken, &source.name);
    inventory::apply(db, config, &source, plan, None)
}

/// What `bssh teams agent add` produced.
#[derive(Debug)]
pub struct AgentCreated {
    pub info: bssh_teams_proto::api::AgentInfo,
    pub credential_path: std::path::PathBuf,
    pub fingerprint: String,
    pub policy: String,
}

/// The JSON text of local policy `name`, as it will be signed.
fn local_policy_json(config: &AppConfig, name: &str) -> Result<String> {
    let policy = config
        .ai
        .policies
        .get(name)
        .cloned()
        .or_else(|| (name == "default").then(Default::default))
        .ok_or_else(|| anyhow!("no policy '{name}' in config.json (ai.policies)"))?;
    Ok(serde_json::to_string(&policy)?)
}

fn device_peer(label: &str, account_id: Uuid, d: &DeviceInfo) -> Peer {
    Peer {
        device_id: d.device_id,
        account_id,
        label: label.to_string(),
        ed25519: d.ed25519_public,
        x25519: d.x25519_public,
    }
}

/// Live devices of the team's human admins.
fn admin_peers(members: &[Member]) -> Vec<Peer> {
    members
        .iter()
        .filter(|m| m.role == Role::Admin && m.kind == MemberKind::Human)
        .flat_map(|m| {
            m.devices
                .iter()
                .filter(|d| d.revoked_at.is_none())
                .map(|d| device_peer(&m.email, m.account_id, d))
        })
        .collect()
}

impl<'a> Session<'a> {
    pub fn client(&self) -> &TeamsClient {
        &self.client
    }

    /// The decrypted vault, or `None` while this device has no team key.
    pub async fn vault_view(&self, team_id: Uuid) -> Result<Option<VaultView>> {
        let info = self.team_info(team_id).await?;
        let Some(codec) = self.codec(team_id, info.current_key_version).await? else {
            return Ok(None);
        };
        Ok(Some(read_vault(self.store, &codec, team_id)?))
    }

    fn relay_endpoint(&self, team_id: Uuid) -> Result<RelayEndpoint> {
        let client = TeamsClient::new(
            &self.identity.server_url,
            self.identity.device_id,
            self.keys.clone(),
        )?;
        Ok(RelayEndpoint {
            team_id,
            device_id: self.identity.device_id,
            keys: self.keys.clone(),
            transport: Arc::new(client),
        })
    }

    /// Create an agent identity (admin): its own device keys, a seat, the team
    /// key, a 0600 credential file for the gateway and, signed by this admin,
    /// its policy.
    pub async fn agent_add(
        &self,
        team_id: Uuid,
        team_name: &str,
        name: &str,
        policy: Option<&str>,
    ) -> Result<AgentCreated> {
        let info = self.require_admin(team_id).await?;
        if info.rotation_required {
            bail!("a key rotation is pending: run `bssh teams sync {team_name}` first");
        }
        let policy_name = policy.unwrap_or("default").to_string();
        let policy_json = local_policy_json(self.config, &policy_name)?;
        let path = credential_path(self.config, team_name, name);
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        let local = self.local_team(team_id)?;
        let key = self
            .team_keys(team_id, info.current_key_version)
            .await?
            .into_iter()
            .find(|k| k.key_version() == info.current_key_version)
            .ok_or_else(|| anyhow!("this device has no team key yet"))?;

        let keys = DeviceKeys::generate();
        let created = self
            .client
            .create_agent(
                team_id,
                &bssh_teams_proto::api::CreateAgentRequest {
                    name: name.to_string(),
                    device: keys.to_new_device(name),
                },
            )
            .await?;
        let wrapped = crypto::wrap_team_key(
            team_id,
            &key,
            &created.device,
            self.identity.device_id,
            &self.keys,
        )?;
        self.client
            .put_keys(
                team_id,
                &PutKeysRequest {
                    keys: vec![wrapped],
                },
            )
            .await?;
        self.approve_device(team_id, &created.device, &format!("agent:{name}"))?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;

        let cred = AgentCredential::new(
            self.identity.server_url.clone(),
            team_id,
            team_name.to_string(),
            name.to_string(),
            created.account_id,
            created.device.device_id,
            &keys,
            local.trust_root_ed25519,
            admin_peers(&members),
            policy_name.clone(),
        );
        cred.save(&path)?;
        self.set_agent_policy(team_id, created.device.device_id, &policy_name, policy_json)
            .await?;
        Ok(AgentCreated {
            info: created,
            credential_path: path,
            fingerprint: fingerprint(&keys.ed25519_public()),
            policy: policy_name,
        })
    }

    async fn set_agent_policy(
        &self,
        team_id: Uuid,
        agent_device_id: Uuid,
        policy_name: &str,
        policy_json: String,
    ) -> Result<u64> {
        let existing = self.vault_view(team_id).await?.and_then(|v| {
            v.agent_policies.into_iter().find(|p| {
                p.policy.agent_device_id == agent_device_id && p.policy.name == policy_name
            })
        });
        let version = existing.as_ref().map_or(1, |p| p.policy.version + 1);
        let mut signed = SignedAgentPolicy {
            agent_device_id,
            name: policy_name.to_string(),
            policy_json,
            version,
            signed_at: Utc::now(),
            signer_device_id: self.identity.device_id,
            signature: Fixed::ZERO,
        };
        crypto::sign_policy(team_id, &mut signed, &self.keys);
        self.sync_team(
            team_id,
            vec![Edit::AgentPolicy {
                entry_id: existing.map(|p| p.entry_id),
                policy: signed,
            }],
        )
        .await?;
        Ok(version)
    }

    /// Re-sign `policy_name` (from config.json) for an existing agent.
    pub async fn agent_set_policy(
        &self,
        team_id: Uuid,
        name: &str,
        policy_name: &str,
    ) -> Result<u64> {
        self.require_admin(team_id).await?;
        let json = local_policy_json(self.config, policy_name)?;
        let agent = self.find_agent(team_id, name).await?;
        self.set_agent_policy(team_id, agent.device.device_id, policy_name, json)
            .await
    }

    async fn find_agent(
        &self,
        team_id: Uuid,
        name: &str,
    ) -> Result<bssh_teams_proto::api::AgentInfo> {
        self.client
            .list_agents(team_id)
            .await?
            .into_iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow!("no agent named '{name}' in this team"))
    }

    pub async fn agent_list(&self, team_id: Uuid) -> Result<Vec<bssh_teams_proto::api::AgentInfo>> {
        Ok(self.client.list_agents(team_id).await?)
    }

    /// Remove an agent: frees its seat, rotates the team key (it held it).
    pub async fn agent_remove(&self, team_id: Uuid, name: &str) -> Result<TeamSync> {
        let agent = self.find_agent(team_id, name).await?;
        let members = self.client.list_members(team_id).await?;
        let email = members
            .iter()
            .find(|m| m.member_id == agent.member_id)
            .map(|m| m.email.clone())
            .ok_or_else(|| anyhow!("agent '{name}' is not in the member list"))?;
        self.remove(team_id, &email).await
    }

    /// Open approval requests addressed to this admin device that it has not decided yet.
    pub async fn pending_approvals(&self, team_id: Uuid) -> Result<Vec<IncomingRequest>> {
        self.require_admin(team_id).await?;
        let ep = self.relay_endpoint(team_id)?;
        let members = self.client.list_members(team_id).await?;
        let admins = admin_peers(&members);
        let agents: Vec<Peer> = self
            .client
            .list_agents(team_id)
            .await?
            .iter()
            .map(|a| device_peer(&a.name, a.account_id, &a.device))
            .collect();
        let (reqs, _) = ep.pending_requests(&agents, 0, 0).await?;
        let done = ep.seen_decisions(&admins).await?;
        Ok(reqs
            .into_iter()
            .filter(|r| {
                !done.iter().any(|(req, acct)| {
                    *req == r.relay_request_id && *acct == self.identity.account_id
                })
            })
            .collect())
    }

    /// Approve or deny the pending request whose id starts with `prefix`.
    pub async fn decide_approval(
        &self,
        team_id: Uuid,
        prefix: &str,
        approve: bool,
        reason: Option<String>,
    ) -> Result<IncomingRequest> {
        let pending = self.pending_approvals(team_id).await?;
        let matches: Vec<_> = pending
            .iter()
            .filter(|r| {
                r.payload.request.request_id.starts_with(prefix)
                    || r.relay_request_id.to_string().starts_with(prefix)
            })
            .collect();
        let req = match matches.as_slice() {
            [one] => (*one).clone(),
            [] => bail!("no pending request starting with '{prefix}'"),
            _ => bail!("'{prefix}' matches several requests; use more of the id"),
        };
        let ep = self.relay_endpoint(team_id)?;
        let members = self.client.list_members(team_id).await?;
        ep.send_decision(
            &req,
            approve,
            &self.identity.email,
            reason,
            None,
            &admin_peers(&members),
        )
        .await?;
        Ok(req)
    }
}

/// Keep the server's wording for billing and organization requests, `Forbidden` and
/// `Conflict` included: those messages name the rule ("only an owner of the
/// organization can manage billing", "the organization must keep at least one
/// owner"), which the generic hints of [`explain`] would hide. Credentials and
/// version errors keep their hint.
fn org_message(e: TeamsError) -> anyhow::Error {
    match &e {
        TeamsError::Api { error, .. }
            if !matches!(
                e.code(),
                Some(ErrorCode::Unauthorized | ErrorCode::ClientTooOld)
            ) =>
        {
            anyhow!("{}", error.message)
        }
        _ => e.into(),
    }
}

/// Map a server error to a short, actionable message for the CLI. Billing
/// and provider errors (`PaymentRequired`, `SeatLimit`, `Internal`) get no
/// hint: the server's own message names the exact fix and is shown as is.
pub fn explain(err: &anyhow::Error) -> Option<&'static str> {
    let code = err.downcast_ref::<TeamsError>()?.code()?;
    Some(match code {
        ErrorCode::Unauthorized => "the server rejected this device's credentials",
        ErrorCode::Forbidden => "you don't have permission for that in this team",
        ErrorCode::Conflict => "that already exists or was already used",
        ErrorCode::ClientTooOld => "the server needs a newer bssh; please upgrade",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_error(status: u16, code: ErrorCode, message: &str) -> TeamsError {
        TeamsError::Api {
            status,
            error: bssh_teams_proto::api::ApiError {
                code,
                message: message.into(),
            },
        }
    }

    #[test]
    fn org_errors_keep_the_server_rule() {
        let forbidden = org_message(api_error(
            403,
            ErrorCode::Forbidden,
            "only an owner of the organization can manage billing",
        ));
        assert_eq!(
            forbidden.to_string(),
            "only an owner of the organization can manage billing"
        );
        assert_eq!(explain(&forbidden), None, "no generic hint over it");

        let last_owner = org_message(api_error(
            409,
            ErrorCode::Conflict,
            "the organization must keep at least one owner",
        ));
        assert_eq!(
            last_owner.to_string(),
            "the organization must keep at least one owner"
        );

        // Credentials keep their actionable hint.
        let unauthorized = org_message(api_error(401, ErrorCode::Unauthorized, "unauthorized"));
        assert!(explain(&unauthorized).is_some());
    }

    fn pk(b: u8) -> PublicKey32 {
        PublicKey32::from([b; 32])
    }

    fn device(n: u8, revoked: bool) -> DeviceInfo {
        DeviceInfo {
            device_id: Uuid::from_u128(n as u128),
            account_id: Uuid::nil(),
            name: format!("dev{n}"),
            x25519_public: pk(n),
            ed25519_public: pk(n + 100),
            created_at: Utc::now(),
            revoked_at: revoked.then(Utc::now),
        }
    }

    fn member(n: u128, email: &str, devices: Vec<DeviceInfo>) -> Member {
        Member {
            kind: Default::default(),
            member_id: Uuid::from_u128(1000 + n),
            account_id: Uuid::from_u128(2000 + n),
            email: email.into(),
            role: Role::Member,
            devices,
            joined_at: Utc::now(),
        }
    }

    fn approve(map: &mut ApprovedDevices, d: &DeviceInfo) {
        map.insert(d.device_id, ("x".into(), d.x25519_public, d.ed25519_public));
    }

    #[test]
    fn plan_splits_recipients_and_blocking() {
        let (a, b, c, gone) = (
            device(1, false),
            device(2, false),
            device(3, false),
            device(4, true),
        );
        let members = vec![
            member(1, "admin@x.io", vec![a.clone()]),
            member(2, "bob@x.io", vec![b.clone(), gone]),
            member(3, "carol@x.io", vec![c.clone()]),
        ];
        let mut approved = HashMap::new();
        approve(&mut approved, &a);
        approve(&mut approved, &b);
        let p = plan_removal(&members, " Bob@X.io ", &approved, 3, 5).unwrap();
        assert_eq!(p.email, "bob@x.io");
        assert_eq!(p.next_key_version, 4);
        assert_eq!(p.shared_hosts, 5);
        assert_eq!(p.removed_devices.len(), 1, "revoked devices are skipped");
        assert!(p.removed_devices[0].approved);
        assert_eq!(p.recipients.len(), 1);
        assert_eq!(p.recipients[0].email, "admin@x.io");
        assert_eq!(p.blocking.len(), 1);
        assert_eq!(p.blocking[0].email, "carol@x.io");
        assert!(!p.blocking[0].approved);
    }

    #[test]
    fn changed_keys_block_rotation() {
        let a = device(1, false);
        let b = device(2, false);
        let members = vec![
            member(1, "a@x.io", vec![a.clone()]),
            member(2, "b@x.io", vec![b]),
        ];
        let mut approved = HashMap::new();
        let mut stale = a.clone();
        stale.x25519_public = pk(99);
        approve(&mut approved, &stale);
        let p = plan_removal(&members, "b@x.io", &approved, 1, 0).unwrap();
        assert!(p.recipients.is_empty());
        assert_eq!(p.blocking.len(), 1);
    }

    #[test]
    fn unknown_member_is_an_error() {
        let members = vec![member(1, "a@x.io", vec![])];
        assert!(plan_removal(&members, "nobody@x.io", &HashMap::new(), 1, 0).is_err());
    }

    // ── Email verification on account creation ──────────────────────────────

    use crate::services::teams::client::mock::mock_server;
    use bssh_teams_proto::api::{CreateAccountResponse, EmailCodeResponse};

    /// Answers `read_code` from a script and records the notices it was shown.
    struct ScriptedPrompt {
        terminal: bool,
        answers: std::collections::VecDeque<Option<&'static str>>,
        notices: Vec<String>,
    }

    impl ScriptedPrompt {
        fn new(terminal: bool, answers: &[Option<&'static str>]) -> Self {
            Self {
                terminal,
                answers: answers.iter().copied().collect(),
                notices: Vec::new(),
            }
        }
    }

    impl EmailCodePrompt for ScriptedPrompt {
        fn terminal(&self) -> bool {
            self.terminal
        }

        fn read_code(&mut self, notice: Option<&str>) -> Result<Option<String>> {
            if let Some(notice) = notice {
                self.notices.push(notice.to_string());
            }
            Ok(self
                .answers
                .pop_front()
                .expect("the code was asked for more often than scripted")
                .map(str::to_string))
        }
    }

    fn local_store(dir: &tempfile::TempDir) -> TeamsStore {
        TeamsStore::open(&AppConfig {
            database_path: dir.path().join("bssh.db"),
            ..Default::default()
        })
        .unwrap()
    }

    fn verification_required() -> String {
        r#"{"code":"email_verification_required","message":"verify your email first"}"#.to_string()
    }

    fn invalid_code() -> String {
        r#"{"code":"invalid_email_code","message":"that code is wrong or expired"}"#.to_string()
    }

    fn code_sent(expires_in_secs: u64) -> String {
        serde_json::to_string(&EmailCodeResponse { expires_in_secs }).unwrap()
    }

    fn account_created() -> String {
        serde_json::to_string(&CreateAccountResponse {
            account_id: Uuid::from_u128(5),
            device_id: Uuid::from_u128(6),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn init_mails_a_code_and_retries_with_it() {
        let (url, seen) = mock_server(|i| match i {
            0 => (403, verification_required()),
            1 => (200, code_sent(900)),
            _ => (200, account_created()),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = local_store(&dir);
        let mut prompt = ScriptedPrompt::new(true, &[Some("123456")]);

        let identity = init_interactive(
            &store,
            &url,
            "Ana@Example.com",
            "laptop",
            "a long passphrase",
            None,
            None,
            &mut prompt,
        )
        .await
        .unwrap();

        assert_eq!(identity.email, "ana@example.com");
        assert_eq!(identity.account_id, Uuid::from_u128(5));
        assert!(load_identity(store.database()).unwrap().is_some());
        assert_eq!(
            prompt.notices,
            ["A verification code was sent to ana@example.com (valid 15 minutes)."]
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3, "required, code mailed, retry: {seen:#?}");
        assert!(
            seen[0].starts_with("POST /v1/accounts HTTP/1.1"),
            "{}",
            seen[0]
        );
        assert!(!seen[0].contains("email_code"), "{}", seen[0]);
        assert!(
            seen[1].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
            "{}",
            seen[1]
        );
        assert!(
            seen[1].contains(r#""email":"ana@example.com""#),
            "{}",
            seen[1]
        );
        assert!(
            seen[2].starts_with("POST /v1/accounts HTTP/1.1"),
            "{}",
            seen[2]
        );
        assert!(seen[2].contains(r#""email_code":"123456""#), "{}", seen[2]);
    }

    #[tokio::test]
    async fn init_sends_a_code_it_was_given() {
        let (url, seen) = mock_server(|_| (200, account_created())).await;
        let dir = tempfile::tempdir().unwrap();
        let store = local_store(&dir);

        init_interactive(
            &store,
            &url,
            "ana@example.com",
            "laptop",
            "a long passphrase",
            None,
            Some("777777"),
            &mut ScriptedPrompt::new(true, &[]),
        )
        .await
        .unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:#?}");
        assert!(seen[0].contains(r#""email_code":"777777""#), "{}", seen[0]);
    }

    #[tokio::test]
    async fn init_without_a_terminal_mails_a_code_and_says_how_to_rerun() {
        let (url, seen) = mock_server(|i| match i {
            0 => (403, verification_required()),
            _ => (200, code_sent(600)),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = local_store(&dir);

        let err = init_interactive(
            &store,
            &url,
            "ana@example.com",
            "laptop",
            "a long passphrase",
            None,
            None,
            &mut ScriptedPrompt::new(false, &[]),
        )
        .await
        .unwrap_err();

        let text = err.to_string();
        assert!(text.contains("no terminal"), "{text}");
        assert!(
            text.contains("a new code was sent to ana@example.com (valid 10 minutes)"),
            "{text}"
        );
        assert!(text.contains("rerun with `--email-code <CODE>`"), "{text}");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "the code was requested: {seen:#?}");
        assert!(
            seen[1].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
            "{}",
            seen[1]
        );
        assert!(load_identity(store.database()).unwrap().is_none());
    }

    #[tokio::test]
    async fn init_reprompts_after_a_refused_code_and_gives_up_after_three() {
        let (url, seen) = mock_server(|i| match i {
            0 => (403, verification_required()),
            1 => (200, code_sent(900)),
            2..=4 => (403, invalid_code()),
            _ => (200, account_created()),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = local_store(&dir);
        let mut prompt =
            ScriptedPrompt::new(true, &[Some("111111"), Some("222222"), Some("333333")]);

        let err = init_interactive(
            &store,
            &url,
            "ana@example.com",
            "laptop",
            "a long passphrase",
            None,
            None,
            &mut prompt,
        )
        .await
        .unwrap_err();

        let text = err.to_string();
        assert!(
            text.contains("refused the verification code 3 times"),
            "{text}"
        );
        assert!(text.contains("`--email-code <CODE>`"), "{text}");
        assert_eq!(prompt.notices.len(), 3, "{:?}", prompt.notices);
        assert_eq!(
            prompt.notices[0],
            "A verification code was sent to ana@example.com (valid 15 minutes)."
        );
        assert_eq!(
            prompt.notices[1],
            "That code is wrong, expired or already used."
        );
        assert_eq!(prompt.notices[2], prompt.notices[1]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 5, "one code mailed, three refused: {seen:#?}");
        // Nothing half-set-up blocks the rerun the error asks for.
        assert!(load_identity(store.database()).unwrap().is_none());
    }

    #[tokio::test]
    async fn an_empty_line_mails_a_new_code() {
        let (url, seen) = mock_server(|i| match i {
            0 => (403, verification_required()),
            1 | 2 => (200, code_sent(60)),
            _ => (200, account_created()),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = local_store(&dir);
        let mut prompt = ScriptedPrompt::new(true, &[None, Some("424242")]);

        init_interactive(
            &store,
            &url,
            "ana@example.com",
            "laptop",
            "a long passphrase",
            None,
            None,
            &mut prompt,
        )
        .await
        .unwrap();

        assert_eq!(prompt.notices.len(), 2);
        assert!(
            prompt
                .notices
                .iter()
                .all(|n| n.ends_with("(valid 1 minute).")),
            "{:?}",
            prompt.notices
        );
        let seen = seen.lock().unwrap();
        let mailed = seen
            .iter()
            .filter(|h| h.starts_with("POST /v1/accounts/email-code HTTP/1.1"))
            .count();
        assert_eq!(mailed, 2, "the empty line mailed one more: {seen:#?}");
    }
}
