//! Team workflows: create, sync, share, invite, join, grants, members, audit.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use super::dto::{
    AuditLineDto, DeviceDto, InviteDto, JoinDto, MemberDto, PendingDto, PendingInviteDto,
    TeamAttentionDto, TeamAuditDto, TeamRemovePreviewDto, TeamSyncDto, TeamSyncResult,
};
use super::org::parse_org;
use super::state::{message, open_store, with_session, TeamsState};
use crate::commands::CommandError;
use bayesian_ssh::services::teams::audit::EventCheck;
use bayesian_ssh::services::teams::crypto::fingerprint;
use bayesian_ssh::services::teams::session::{self, AuditQuery, JoinWith};
use bssh_teams_proto::api::AuditKind;
use bssh_teams_proto::api::Role;
use chrono::{DateTime, Utc};
use tauri::State;

pub(super) fn resolve(team: &str) -> anyhow::Result<uuid::Uuid> {
    let (store, _) = open_store()?;
    Ok(session::resolve_team(store.database(), team)?.0)
}

/// Create a team. `org` is an organization id the caller owns; without it the
/// server picks the oldest organization the caller owns (or makes one).
#[tauri::command(async)]
pub fn teams_create(
    state: State<'_, TeamsState>,
    name: String,
    org: Option<String>,
) -> Result<TeamSyncDto, CommandError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Team name cannot be empty.".into());
    }
    let org_id = org.as_deref().map(parse_org).transpose()?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { Ok(TeamSyncDto::from(&s.create_team_in(&name, org_id).await?)) })
    })
}

#[tauri::command(async)]
pub fn teams_sync(
    state: State<'_, TeamsState>,
    team: Option<String>,
) -> Result<Vec<TeamSyncResult>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let (store, _) = open_store()?;
            let targets = match team {
                Some(t) => vec![session::resolve_team(store.database(), &t)?],
                None => session::local_teams(store.database())?,
            };
            let mut out = Vec::new();
            for (team_id, source) in targets {
                let (result, error) = match s.sync_team(team_id, Vec::new()).await {
                    Ok(sync) => (Some(TeamSyncDto::from(&sync)), None),
                    Err(e) => (None, Some(message(e))),
                };
                out.push(TeamSyncResult {
                    team: source.name,
                    result,
                    error,
                });
            }
            Ok(out)
        })
    })
}

#[tauri::command(async)]
pub fn teams_share(
    state: State<'_, TeamsState>,
    connection: String,
    team: String,
) -> Result<TeamSyncDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let (store, _) = open_store()?;
            let conn = store
                .database()
                .connection_or_alias(&connection)?
                .ok_or_else(|| anyhow::anyhow!("no connection named '{connection}'"))?;
            Ok(TeamSyncDto::from(&s.share(team_id, &conn).await?.1))
        })
    })
}

#[tauri::command(async)]
pub fn teams_unshare(
    state: State<'_, TeamsState>,
    connection: String,
) -> Result<TeamSyncDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let (store, _) = open_store()?;
            let conn = store
                .database()
                .connection_or_alias(&connection)?
                .ok_or_else(|| anyhow::anyhow!("no connection named '{connection}'"))?;
            Ok(TeamSyncDto::from(&s.unshare(&conn).await?))
        })
    })
}

#[tauri::command(async)]
pub fn teams_invite(
    state: State<'_, TeamsState>,
    team: String,
    email: String,
    admin: bool,
) -> Result<InviteDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let role = if admin { Role::Admin } else { Role::Member };
            let (invite, admin_fingerprint) = s.invite(team_id, &email, role).await?;
            Ok(InviteDto {
                emailed: invite.emailed,
                email: invite.email,
                expires_at: invite.expires_at,
                token: invite
                    .token
                    .ok_or_else(|| anyhow::anyhow!("the server didn't return an invite token"))?,
                admin_fingerprint,
                server_url: s.identity.server_url.clone(),
            })
        })
    })
}

#[tauri::command(async)]
pub fn teams_join(
    state: State<'_, TeamsState>,
    token: Option<String>,
    team_id: Option<String>,
    fingerprint: String,
) -> Result<JoinDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team = team_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(uuid::Uuid::parse_str)
                .transpose()
                .map_err(|e| anyhow::anyhow!("invalid team id: {e}"))?;
            let with = match (token.as_deref().filter(|t| !t.trim().is_empty()), team) {
                (Some(token), _) => JoinWith::Token(token),
                (None, Some(team)) => JoinWith::Team(team),
                (None, None) => {
                    anyhow::bail!("Paste the invite token, or the team id to finish a join.")
                }
            };
            let (team_id, name) = s.join(with, &fingerprint).await?;
            Ok(JoinDto {
                team_id: team_id.to_string(),
                name,
                my_fingerprint: s.my_fingerprint(),
            })
        })
    })
}

#[tauri::command(async)]
pub fn teams_pending(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<Vec<PendingDto>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            Ok(s.pending(team_id)
                .await?
                .into_iter()
                .map(|p| PendingDto {
                    email: p.email,
                    device: DeviceDto {
                        name: p.device.name,
                        fingerprint: p.fingerprint,
                    },
                })
                .collect())
        })
    })
}

#[tauri::command(async)]
pub fn teams_grant(
    state: State<'_, TeamsState>,
    team: String,
    fingerprint: String,
) -> Result<PendingDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let p = s.grant(team_id, &fingerprint).await?;
            Ok(PendingDto {
                email: p.email,
                device: DeviceDto {
                    name: p.device.name,
                    fingerprint: p.fingerprint,
                },
            })
        })
    })
}

#[tauri::command(async)]
pub fn teams_members(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<Vec<MemberDto>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            Ok(s.members(team_id)
                .await?
                .into_iter()
                .map(|m| MemberDto {
                    email: m.email,
                    role: match m.role {
                        Role::Admin => "admin",
                        Role::Member => "member",
                    },
                    devices: m
                        .devices
                        .into_iter()
                        .filter(|d| d.revoked_at.is_none())
                        .map(|d| DeviceDto {
                            fingerprint: fingerprint(&d.ed25519_public),
                            name: d.name,
                        })
                        .collect(),
                })
                .collect())
        })
    })
}

/// Pending invites (admin); empty for a member.
#[tauri::command(async)]
pub fn teams_invites(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<Vec<PendingInviteDto>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            Ok(s.invites(resolve(&team)?)
                .await?
                .unwrap_or_default()
                .into_iter()
                .map(|i| PendingInviteDto {
                    email: i.email,
                    role: match i.role {
                        Role::Admin => "admin",
                        Role::Member => "member",
                    },
                    expires_at: i.expires_at,
                })
                .collect())
        })
    })
}

#[tauri::command(async)]
pub fn teams_uninvite(
    state: State<'_, TeamsState>,
    team: String,
    email: String,
) -> Result<(), CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            s.uninvite(resolve(&team)?, &email).await?;
            Ok(())
        })
    })
}

#[tauri::command(async)]
pub fn teams_remove(
    state: State<'_, TeamsState>,
    team: String,
    email: String,
) -> Result<TeamSyncDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            Ok(TeamSyncDto::from(&s.remove(team_id, &email).await?))
        })
    })
}

/// What needs a human in each local team. A team that can't be reached is
/// skipped so one offline server doesn't hide the others.
#[tauri::command(async)]
pub fn teams_attention(
    state: State<'_, TeamsState>,
) -> Result<Vec<TeamAttentionDto>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let (store, _) = open_store()?;
            let mut out = Vec::new();
            for (team_id, _) in session::local_teams(store.database())? {
                if let Ok(a) = s.attention(team_id).await {
                    out.push(TeamAttentionDto {
                        team_id: team_id.to_string(),
                        role: match a.role {
                            Role::Admin => "admin",
                            Role::Member => "member",
                        },
                        pending_devices: a.pending_devices,
                        waiting_for_key: a.waiting_for_key,
                        removed: a.removed,
                    });
                }
            }
            Ok(out)
        })
    })
}

#[tauri::command(async)]
pub fn teams_remove_preview(
    state: State<'_, TeamsState>,
    team: String,
    email: String,
) -> Result<TeamRemovePreviewDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let p = s.remove_preview(team_id, &email).await?;
            Ok(TeamRemovePreviewDto {
                email: p.email,
                removed_devices: p.removed_devices.into_iter().map(Into::into).collect(),
                next_key_version: p.next_key_version,
                recipients: p.recipients.into_iter().map(Into::into).collect(),
                blocking: p.blocking.into_iter().map(Into::into).collect(),
                shared_hosts: p.shared_hosts,
            })
        })
    })
}

#[tauri::command(async)]
pub fn teams_audit(
    state: State<'_, TeamsState>,
    team: String,
    host: Option<String>,
    member: Option<String>,
    since: Option<String>,
) -> Result<TeamAuditDto, CommandError> {
    let since = since
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| {
            DateTime::parse_from_rfc3339(v)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|e| {
                    CommandError::from(format!("'since' must be an RFC 3339 date-time: {e}"))
                })
        })
        .transpose()?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let entry_id = host
                .as_deref()
                .filter(|h| !h.trim().is_empty())
                .map(|h| s.entry_for_connection(team_id, h))
                .transpose()?;
            let log = s
                .audit(
                    team_id,
                    AuditQuery {
                        entry_id,
                        email: member.filter(|m| !m.trim().is_empty()),
                        since,
                        // The window this view always showed; the whole log
                        // is verified regardless.
                        limit: Some(1000),
                        ..Default::default()
                    },
                )
                .await?;
            Ok(TeamAuditDto {
                clean: log.is_clean(),
                chains: log.chains.len(),
                verified_events: log.verified_events,
                problems: log.problems.clone(),
                invalid_events: log.invalid_events,
                complete: log.complete,
                lines: log
                    .lines
                    .into_iter()
                    .map(|l| AuditLineDto {
                        occurred_at: l.occurred_at,
                        received_at: l.received_at,
                        kind: match l.kind {
                            AuditKind::SessionStart => "session_start",
                            AuditKind::SessionEnd => "session_end",
                            AuditKind::VaultRead => "vault_read",
                            AuditKind::VaultWrite => "vault_write",
                            // bssh AI kinds: shown by their wire names.
                            other => other.as_str(),
                        },
                        who: l.who,
                        device_name: l.device_name,
                        host: l.host,
                        client_ip: l.client_ip,
                        check: match l.check {
                            EventCheck::Valid => "valid",
                            EventCheck::BadSignature => "bad_signature",
                            EventCheck::UnknownDevice => "unknown_device",
                            EventCheck::WrongTeam => "wrong_team",
                        },
                    })
                    .collect(),
            })
        })
    })
}
