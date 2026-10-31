//! Syncing a team's vault and sharing local connections into it.

use super::handle::Session;
use super::local::{link_to_team_source, local_teams, materialize_into, team_of_connection};
use super::types::{Edit, TeamSync};
use crate::models::Connection;
use crate::services::inventory::sync as inventory;
use crate::services::teams::sync::SyncEngine;
use crate::services::teams::{VaultView, queue_connection, queue_delete, queue_edit, read_vault};
use anyhow::{Result, anyhow, bail};
use bssh_teams_proto::api::{ConnectionRecord, Role, VaultPayload};
use uuid::Uuid;

impl<'a> Session<'a> {
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
    ///
    /// A local connection becomes the team's: it is linked to the vault entry
    /// before the sync, so the sync updates it instead of adding a renamed copy
    /// (`name@team`), and teammates' later edits reach the connection the user
    /// actually uses, behind the usual confirmation of route changes. A copy
    /// left by an earlier share is adopted: its entry is reused and the copy
    /// removed.
    pub async fn share(&self, team_id: Uuid, connection: &Connection) -> Result<(Uuid, TeamSync)> {
        let (entry_id, unlinked) = match team_of_connection(self.db(), connection)? {
            Some((t, entry)) if t == team_id => (entry, None),
            Some(_) => bail!("'{}' belongs to another team", connection.name),
            None => (self.link_to_team(team_id, connection)?, Some(connection)),
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
        let sync = match self
            .sync_team(
                team_id,
                vec![Edit::Connection {
                    entry_id: Some(entry_id),
                    record,
                }],
            )
            .await
        {
            Ok(sync) => sync,
            Err(e) => {
                // Unlink, or the next sync would find no entry for it and mark
                // the user's own connection stale.
                if let Some(original) = unlinked {
                    self.db().update_connection(original)?;
                }
                return Err(e);
            }
        };
        Ok((entry_id, sync))
    }

    /// Link a local connection to `team_id`'s source: see [`link_to_team_source`].
    fn link_to_team(&self, team_id: Uuid, connection: &Connection) -> Result<Uuid> {
        let (_, source) = local_teams(self.db())?
            .into_iter()
            .find(|(id, _)| *id == team_id)
            .ok_or_else(|| anyhow!("team {team_id} has no local source"))?;
        link_to_team_source(self.db(), &source, connection)
    }

    /// Delete a team connection from the vault for everyone. This machine
    /// keeps it as a local connection.
    pub async fn unshare(&self, connection: &Connection) -> Result<TeamSync> {
        let (team_id, entry_id) = team_of_connection(self.db(), connection)?
            .ok_or_else(|| anyhow!("'{}' is not a team connection", connection.name))?;
        let sync = self
            .sync_team(team_id, vec![Edit::Delete(entry_id)])
            .await?;
        // Gone for the team; here it goes back to being a local connection (a
        // shared connection is often the sharer's own), not a stale one.
        if let Some(mut local) = self.db().connection(&connection.id.to_string())? {
            local.source_id = None;
            local.external_id = None;
            local.stale_since = None;
            local.pending_change = None;
            self.db().update_connection(&local)?;
        }
        Ok(sync)
    }

    /// The entry id of a team connection by its local name or alias.
    pub fn entry_for_connection(&self, team_id: Uuid, name: &str) -> Result<Uuid> {
        let conn = self
            .db()
            .connection_or_alias(name)?
            .ok_or_else(|| anyhow!("no connection named '{name}'"))?;
        match team_of_connection(self.db(), &conn)? {
            Some((t, entry)) if t == team_id => Ok(entry),
            _ => bail!("'{name}' is not a connection of this team"),
        }
    }

    /// The decrypted vault, or `None` while this device has no team key.
    pub async fn vault_view(&self, team_id: Uuid) -> Result<Option<VaultView>> {
        let info = self.team_info(team_id).await?;
        let Some(codec) = self.codec(team_id, info.current_key_version).await? else {
            return Ok(None);
        };
        Ok(Some(read_vault(self.store, &codec, team_id)?))
    }
}
