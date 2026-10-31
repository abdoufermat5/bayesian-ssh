//! Member removal: the preview, and the key rotation that follows.

use super::handle::Session;
use super::local::{kind_from_str, team_id_of, uuid};
use super::types::TeamSync;
use crate::services::teams::codec::CryptoCodec;
use crate::services::teams::crypto::{self, TeamKey, fingerprint};
use crate::services::teams::{VaultCodec, queue_edit};
use anyhow::{Context, Result, anyhow, bail};
use bssh_teams_proto::api::{DeviceInfo, Member, PutKeysRequest, Role, TeamInfo};
use bssh_teams_proto::wire::PublicKey32;
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::collections::HashMap;
use uuid::Uuid;

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

pub(super) type ApprovedDevices = HashMap<Uuid, (String, PublicKey32, PublicKey32)>;

/// `device` as listed in a preview, not yet marked approved.
fn preview_device(member: &Member, device: &DeviceInfo) -> PreviewDevice {
    PreviewDevice {
        email: member.email.clone(),
        name: device.name.clone(),
        fingerprint: fingerprint(&device.ed25519_public),
        approved: false,
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
        .map(|d| PreviewDevice {
            approved: approved.contains_key(&d.device_id),
            ..preview_device(removed, d)
        })
        .collect();
    let mut recipients = Vec::new();
    let mut blocking = Vec::new();
    for member in members.iter().filter(|m| m.member_id != removed.member_id) {
        for device in member.devices.iter().filter(|d| d.revoked_at.is_none()) {
            let ok = matches!(
                approved.get(&device.device_id),
                Some((_, x, e)) if *x == device.x25519_public && *e == device.ed25519_public
            );
            let line = PreviewDevice {
                approved: ok,
                ..preview_device(member, device)
            };
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

impl<'a> Session<'a> {
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
    pub(super) async fn rotate(
        &self,
        team_id: Uuid,
        info: &TeamInfo,
        old: &CryptoCodec,
    ) -> Result<u32> {
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

    pub(super) fn sign_pending_audit(&self, team_id: Uuid, codec: &CryptoCodec) -> Result<()> {
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
}
