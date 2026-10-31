//! Team administration: creation, invites, device approval and members.

use super::handle::Session;
use super::removal::TeamAttentionState;
use super::types::{Edit, JoinWith, PendingDevice, TeamSync};
use crate::models::{Source, SourceKind};
use crate::services::teams::client::TeamsError;
use crate::services::teams::crypto::{self, fingerprint, TeamKey};
use crate::services::teams::store::TeamLocal;
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::{
    AcceptInviteRequest, CreateInviteRequest, CreateTeamRequest, ErrorCode, Invite, Member,
    PutKeysRequest, Role,
};
use chrono::Utc;
use uuid::Uuid;

impl<'a> Session<'a> {
    /// Create a team named `name` with this device as its admin and trust root.
    pub async fn create_team(&self, name: &str) -> Result<TeamSync> {
        self.create_team_in(name, None).await
    }

    /// Create a team in `org_id` (the caller must own it). With `None` the
    /// server puts it in the oldest organization the caller owns, or in a new
    /// one.
    pub async fn create_team_in(&self, name: &str, org_id: Option<Uuid>) -> Result<TeamSync> {
        if self.db().source(name)?.is_some() {
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
        if self.db().source(&name)?.is_none() {
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

    // -- organizations -------------------------------------------------------

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
        let info = match self.client.team(team_id).await {
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
}
