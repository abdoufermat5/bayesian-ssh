//! The workflows behind `bssh teams`: identity setup, teams, invites, device
//! approval, sync and key rotation, built on the client, codec, store and sync
//! engine.
//!
//! Trust rule enforced here: the server never decides which devices receive a
//! team key. An admin grants a key only to a device whose fingerprint they
//! were given out of band (`grant`), and a rotation wraps the new key only to
//! devices this machine approved. A server that lists an extra device makes
//! the rotation fail instead of leaking the key.

use super::client::{validate_server_url, TeamsClient, TeamsError};
use super::codec::{unwrap_all, CryptoCodec};
use super::crypto::{self, fingerprint, DeviceKeys, TeamKey};
use super::store::{TeamLocal, TeamsStore};
use super::sync::{SyncEngine, SyncReport};
use super::{queue_connection, queue_delete, queue_edit, read_vault, VaultCodec, VaultView};
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, DiscoveredHost, Source, SourceKind};
use crate::services::inventory::sync as inventory;
use anyhow::{anyhow, bail, Context, Result};
use bssh_teams_proto::api::{
    AcceptInviteRequest, AuditKind, ConnectionRecord, CreateAccountRequest, CreateInviteRequest,
    CreateTeamRequest, DeviceInfo, ErrorCode, Invite, Member, PutKeysRequest, Role, TeamInfo,
    VaultPayload,
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

fn kind_str(kind: AuditKind) -> &'static str {
    match kind {
        AuditKind::SessionStart => "session_start",
        AuditKind::SessionEnd => "session_end",
        AuditKind::VaultRead => "vault_read",
        AuditKind::VaultWrite => "vault_write",
    }
}

fn kind_from_str(s: &str) -> Option<AuditKind> {
    Some(match s {
        "session_start" => AuditKind::SessionStart,
        "session_end" => AuditKind::SessionEnd,
        "vault_read" => AuditKind::VaultRead,
        "vault_write" => AuditKind::VaultWrite,
        _ => return None,
    })
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
pub async fn init(
    store: &TeamsStore,
    server_url: &str,
    email: &str,
    device_name: &str,
    passphrase: &str,
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
        })
        .await?;
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

fn team_of_connection(db: &Database, connection: &Connection) -> Result<Option<(Uuid, Uuid)>> {
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
        if self.db().get_source(name)?.is_some() {
            bail!("a source named '{name}' already exists; pick another team name");
        }
        let team_id = Uuid::new_v4();
        let me = self.my_device();
        let tk = TeamKey::generate(1);
        let initial_key =
            crypto::wrap_team_key(team_id, &tk, &me, self.identity.device_id, &self.keys)?;
        self.client
            .create_team(&CreateTeamRequest { initial_key })
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

    pub async fn members(&self, team_id: Uuid) -> Result<Vec<Member>> {
        self.team_info(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;
        Ok(members)
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
        self.sync_team(team_id, Vec::new()).await
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
        let db = self.db();
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
        inventory::apply(db, self.config, &source, plan, None)
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
}

/// Map a server error to a short, actionable message for the CLI.
pub fn explain(err: &anyhow::Error) -> Option<&'static str> {
    let code = err.downcast_ref::<TeamsError>()?.code()?;
    Some(match code {
        ErrorCode::SeatLimit => "the team has no free seat left",
        ErrorCode::Unauthorized => "the server rejected this device's credentials",
        ErrorCode::Forbidden => "you don't have permission for that in this team",
        ErrorCode::Conflict => "that already exists or was already used",
        ErrorCode::ClientTooOld => "the server needs a newer bssh; please upgrade",
        _ => return None,
    })
}
