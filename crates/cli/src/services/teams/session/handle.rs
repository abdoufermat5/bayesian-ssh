//! The unlocked [`Session`] and the lookups every workflow shares: local team
//! rows, authors, approved devices and team keys.

use super::identity::{require_identity, unlock, Identity};
use super::local::{key32, uuid};
use crate::config::AppConfig;
use crate::database::Database;
use crate::services::teams::client::TeamsClient;
use crate::services::teams::codec::{unwrap_all, CryptoCodec};
use crate::services::teams::crypto::{fingerprint, DeviceKeys, TeamKey};
use crate::services::teams::store::{TeamLocal, TeamsStore};
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::{DeviceInfo, ErrorCode, Member, Role, TeamInfo};
use bssh_teams_proto::wire::PublicKey32;
use chrono::Utc;
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// An unlocked identity: device keys in memory and an authenticated client.
pub struct Session<'a> {
    pub(super) store: &'a TeamsStore,
    pub(super) config: &'a AppConfig,
    pub identity: Identity,
    pub(super) keys: Arc<DeviceKeys>,
    pub(super) client: TeamsClient,
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

    pub(super) fn db(&self) -> &Database {
        self.store.database()
    }

    /// This device's fingerprint, to send to an admin for `bssh teams grant`.
    pub fn my_fingerprint(&self) -> String {
        fingerprint(&self.keys.ed25519_public())
    }

    pub(super) fn my_device(&self) -> DeviceInfo {
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

    pub(super) fn local_team(&self, team_id: Uuid) -> Result<TeamLocal> {
        self.store
            .team(team_id)?
            .ok_or_else(|| anyhow!("team {team_id} is not set up on this machine"))
    }

    pub(super) async fn team_info(&self, team_id: Uuid) -> Result<TeamInfo> {
        match self.client.team(team_id).await {
            Ok(info) => Ok(info),
            Err(e) if e.code() == Some(ErrorCode::Forbidden) => {
                bail!("you're no longer a member of this team")
            }
            Err(e) => Err(e.into()),
        }
    }

    pub(super) async fn require_admin(&self, team_id: Uuid) -> Result<TeamInfo> {
        let info = self.team_info(team_id).await?;
        if info.my_role != Role::Admin {
            bail!("only a team admin can do that");
        }
        Ok(info)
    }

    // -- local trust state ---------------------------------------------------

    pub(super) fn remember_authors(&self, team_id: Uuid, members: &[Member]) -> Result<()> {
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
    pub(super) fn authors(&self, team_id: Uuid) -> Result<HashMap<Uuid, PublicKey32>> {
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

    pub(super) fn approve_device(
        &self,
        team_id: Uuid,
        device: &DeviceInfo,
        email: &str,
    ) -> Result<()> {
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
    pub(super) fn approved_devices(
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
    pub(super) async fn team_keys(&self, team_id: Uuid, current: u32) -> Result<Vec<TeamKey>> {
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

    pub(super) async fn codec(&self, team_id: Uuid, current: u32) -> Result<Option<CryptoCodec>> {
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

    pub(super) async fn rotated_codec(&self, team_id: Uuid, version: u32) -> Result<CryptoCodec> {
        self.codec(team_id, version)
            .await?
            .ok_or_else(|| anyhow!("rotated team key is not available after posting it"))
    }

    // -- workflows -----------------------------------------------------------

    pub fn client(&self) -> &TeamsClient {
        &self.client
    }
}
