//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::teams::audit::EventCheck;
use bayesian_ssh::services::teams::crypto::{fingerprint, DeviceKeys};
use bayesian_ssh::services::teams::session::{
    self, AuditQuery, JoinWith, PreviewDevice, Session, TeamSync,
};
use bayesian_ssh::services::teams::TeamsStore;
use bssh_teams_proto::api::AuditKind;
use bssh_teams_proto::api::Role;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tauri::State;
use zeroize::Zeroizing;

#[derive(Default)]
pub struct TeamsState {
    keys: Mutex<Option<Arc<DeviceKeys>>>,
}

impl TeamsState {
    fn get(&self) -> Option<Arc<DeviceKeys>> {
        self.keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set(&self, keys: Option<Arc<DeviceKeys>>) {
        *self
            .keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = keys;
    }

    fn unlocked(&self) -> Result<Arc<DeviceKeys>, String> {
        self.get()
            .ok_or_else(|| "Teams is locked: unlock it with your passphrase first.".to_string())
    }
}

fn message(e: anyhow::Error) -> String {
    match session::explain(&e) {
        Some(hint) => format!("{hint} ({e:#})"),
        None => format!("{e:#}"),
    }
}

fn open_store() -> anyhow::Result<(TeamsStore, AppConfig)> {
    let config = AppConfig::load(None)?;
    let store = TeamsStore::open(&config)?;
    Ok((store, config))
}

/// Run `job` on a dedicated thread and wait for it.
fn on_teams_thread<T: Send + 'static>(
    job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    std::thread::spawn(job)
        .join()
        .map_err(|_| "the Teams worker crashed".to_string())?
        .map_err(message)
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread tokio runtime always builds")
        .block_on(future)
}

type SessionFuture<'s, T> = std::pin::Pin<Box<dyn Future<Output = anyhow::Result<T>> + 's>>;

/// Open the store and a session over the unlocked keys, then run `op`.
fn with_session<T, F>(keys: Arc<DeviceKeys>, op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: for<'s> FnOnce(&'s Session<'s>) -> SessionFuture<'s, T> + Send + 'static,
{
    on_teams_thread(move || {
        let (store, config) = open_store()?;
        let session = Session::with_keys(&store, &config, keys)?;
        block_on(op(&session))
    })
}

// ── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IdentityDto {
    email: String,
    server_url: String,
    device_id: String,
}

#[derive(Serialize)]
pub struct TeamDto {
    team_id: String,
    /// The team's inventory source; its connections carry this `source_id`.
    source_id: String,
    name: String,
    hosts: usize,
    last_sync_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct TeamsStatus {
    identity: Option<IdentityDto>,
    unlocked: bool,
    fingerprint: Option<String>,
    teams: Vec<TeamDto>,
}

#[derive(Serialize)]
pub struct TeamSyncDto {
    team_id: String,
    name: Option<String>,
    waiting_for_key: bool,
    pulled: usize,
    pushed: usize,
    conflicts_resolved: usize,
    audit_uploaded: usize,
    dropped_edits: usize,
    rejected: Vec<String>,
    unreadable: usize,
    rotated_to: Option<u32>,
    rotation_required: bool,
    errors: Vec<String>,
    hosts_added: usize,
    hosts_updated: usize,
    hosts_stale: usize,
}

impl From<&TeamSync> for TeamSyncDto {
    fn from(s: &TeamSync) -> Self {
        let r = &s.report;
        Self {
            team_id: s.team_id.to_string(),
            name: s.name.clone(),
            waiting_for_key: s.waiting_for_key,
            pulled: r.pulled,
            pushed: r.pushed,
            conflicts_resolved: r.conflicts_resolved,
            audit_uploaded: r.audit_uploaded,
            dropped_edits: r.dropped_edits.len(),
            rejected: r
                .rejected
                .iter()
                .map(|e| format!("{} rev {}: {}", e.entry_id, e.revision, e.reason))
                .collect(),
            unreadable: s.unreadable,
            rotated_to: s.rotated_to,
            rotation_required: r.rotation_required,
            errors: r.errors.clone(),
            hosts_added: s.hosts.as_ref().map_or(0, |h| h.added),
            hosts_updated: s.hosts.as_ref().map_or(0, |h| h.updated),
            hosts_stale: s.hosts.as_ref().map_or(0, |h| h.stale),
        }
    }
}

#[derive(Serialize)]
pub struct TeamSyncResult {
    team: String,
    result: Option<TeamSyncDto>,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct InviteDto {
    email: String,
    expires_at: DateTime<Utc>,
    token: String,
    admin_fingerprint: String,
    server_url: String,
}

#[derive(Serialize)]
pub struct JoinDto {
    team_id: String,
    name: String,
    my_fingerprint: String,
}

#[derive(Serialize)]
pub struct DeviceDto {
    name: String,
    fingerprint: String,
}

#[derive(Serialize)]
pub struct PendingDto {
    email: String,
    device: DeviceDto,
}

/// A pending invite as an admin sees it: no token.
#[derive(Serialize)]
pub struct PendingInviteDto {
    email: String,
    role: &'static str,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MemberDto {
    email: String,
    role: &'static str,
    devices: Vec<DeviceDto>,
}

#[derive(Serialize)]
pub struct AuditLineDto {
    occurred_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
    kind: &'static str,
    who: String,
    device_name: String,
    host: Option<String>,
    client_ip: Option<String>,
    /// "valid", "bad_signature", "unknown_device" or "wrong_team".
    check: &'static str,
}

#[derive(Serialize)]
pub struct TeamAttentionDto {
    team_id: String,
    role: &'static str,
    pending_devices: usize,
    waiting_for_key: bool,
    removed: bool,
}

#[derive(Serialize)]
pub struct PreviewDeviceDto {
    email: String,
    name: String,
    fingerprint: String,
    approved: bool,
}

impl From<PreviewDevice> for PreviewDeviceDto {
    fn from(d: PreviewDevice) -> Self {
        Self {
            email: d.email,
            name: d.name,
            fingerprint: d.fingerprint,
            approved: d.approved,
        }
    }
}

#[derive(Serialize)]
pub struct TeamRemovePreviewDto {
    email: String,
    removed_devices: Vec<PreviewDeviceDto>,
    next_key_version: u32,
    recipients: Vec<PreviewDeviceDto>,
    blocking: Vec<PreviewDeviceDto>,
    shared_hosts: usize,
}

#[derive(Serialize)]
pub struct TeamAuditDto {
    lines: Vec<AuditLineDto>,
    chains: usize,
    from_genesis: bool,
    problems: Vec<String>,
    invalid_events: usize,
    complete: bool,
    clean: bool,
}

// ── Commands ────────────────────────────────────────────────────────────────

fn status(state: &TeamsState) -> anyhow::Result<TeamsStatus> {
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
pub fn teams_status(state: State<'_, TeamsState>) -> Result<TeamsStatus, String> {
    status(&state).map_err(message)
}

fn default_device_name() -> String {
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
) -> Result<TeamsStatus, String> {
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.chars().count() < 8 {
        return Err("Use a passphrase of at least 8 characters.".into());
    }
    let name = device_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(default_device_name);
    let keys = on_teams_thread(move || {
        let (store, _) = open_store()?;
        block_on(session::init(
            &store,
            &server,
            &email,
            &name,
            &passphrase,
            invite_token.as_deref(),
        ))?;
        session::unlock(&store, &passphrase)
    })?;
    state.set(Some(keys));
    status(&state).map_err(message)
}

#[tauri::command(async)]
pub fn teams_unlock(
    state: State<'_, TeamsState>,
    passphrase: String,
) -> Result<TeamsStatus, String> {
    let passphrase = Zeroizing::new(passphrase);
    let keys = on_teams_thread(move || {
        let (store, _) = open_store()?;
        session::unlock(&store, &passphrase)
    })?;
    state.set(Some(keys));
    status(&state).map_err(message)
}

#[tauri::command(async)]
pub fn teams_lock(state: State<'_, TeamsState>) {
    state.set(None);
}

fn resolve(team: &str) -> anyhow::Result<uuid::Uuid> {
    let (store, _) = open_store()?;
    Ok(session::resolve_team(store.database(), team)?.0)
}

#[tauri::command(async)]
pub fn teams_create(state: State<'_, TeamsState>, name: String) -> Result<TeamSyncDto, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Team name cannot be empty.".into());
    }
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { Ok(TeamSyncDto::from(&s.create_team(&name).await?)) })
    })
}

#[tauri::command(async)]
pub fn teams_sync(
    state: State<'_, TeamsState>,
    team: Option<String>,
) -> Result<Vec<TeamSyncResult>, String> {
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
) -> Result<TeamSyncDto, String> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let (store, _) = open_store()?;
            let conn = store
                .database()
                .get_connection_or_alias(&connection)?
                .ok_or_else(|| anyhow::anyhow!("no connection named '{connection}'"))?;
            Ok(TeamSyncDto::from(&s.share(team_id, &conn).await?.1))
        })
    })
}

#[tauri::command(async)]
pub fn teams_unshare(
    state: State<'_, TeamsState>,
    connection: String,
) -> Result<TeamSyncDto, String> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let (store, _) = open_store()?;
            let conn = store
                .database()
                .get_connection_or_alias(&connection)?
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
) -> Result<InviteDto, String> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let role = if admin { Role::Admin } else { Role::Member };
            let (invite, admin_fingerprint) = s.invite(team_id, &email, role).await?;
            Ok(InviteDto {
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
) -> Result<JoinDto, String> {
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
) -> Result<Vec<PendingDto>, String> {
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
) -> Result<PendingDto, String> {
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
pub fn teams_billing(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<bssh_teams_proto::api::BillingInfo, String> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.billing(resolve(&team)?).await })
    })
}

/// A Stripe Checkout URL for the team (admin); the page opens it in the browser.
/// Without `seats`, the seats in use, at least one above the free plan.
#[tauri::command(async)]
pub fn teams_billing_checkout(
    state: State<'_, TeamsState>,
    team: String,
    seats: Option<u32>,
    yearly: bool,
) -> Result<String, String> {
    use bssh_teams_proto::api::BillingInterval;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let seats = match seats {
                Some(n) => n,
                None => {
                    let info = s.billing(team_id).await?;
                    info.seats_used.max(info.free_seats + 1)
                }
            };
            let interval = if yearly {
                BillingInterval::Year
            } else {
                BillingInterval::Month
            };
            s.checkout_url(team_id, seats, interval).await
        })
    })
}

/// The Stripe Customer Portal URL of the team (admin).
#[tauri::command(async)]
pub fn teams_billing_portal(state: State<'_, TeamsState>, team: String) -> Result<String, String> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.portal_url(resolve(&team)?).await })
    })
}

#[tauri::command(async)]
pub fn teams_members(state: State<'_, TeamsState>, team: String) -> Result<Vec<MemberDto>, String> {
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
) -> Result<Vec<PendingInviteDto>, String> {
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
) -> Result<(), String> {
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
) -> Result<TeamSyncDto, String> {
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
pub fn teams_attention(state: State<'_, TeamsState>) -> Result<Vec<TeamAttentionDto>, String> {
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
) -> Result<TeamRemovePreviewDto, String> {
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
) -> Result<TeamAuditDto, String> {
    let since = since
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| {
            DateTime::parse_from_rfc3339(v)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|e| format!("'since' must be an RFC 3339 date-time: {e}"))
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
                        ..Default::default()
                    },
                )
                .await?;
            Ok(TeamAuditDto {
                clean: log.is_clean(),
                chains: log.chains.len(),
                from_genesis: log.chains.iter().all(|c| c.from_genesis()),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The sync engine's futures aren't `Send`; they must still run.
    #[test]
    fn runs_non_send_futures() {
        let out = on_teams_thread(|| {
            Ok(block_on(async {
                let local = std::rc::Rc::new(41);
                tokio::task::yield_now().await;
                *local + 1
            }))
        });
        assert_eq!(out, Ok(42));
    }

    /// Tauri calls commands from inside its own tokio runtime; starting the
    /// worker's runtime there must not panic ("runtime within a runtime").
    #[tokio::test]
    async fn works_from_inside_a_runtime() {
        let out = on_teams_thread(|| Ok(block_on(async { 7 })));
        assert_eq!(out, Ok(7));
    }

    #[test]
    fn worker_errors_become_messages() {
        let out: Result<(), String> = on_teams_thread(|| Err(anyhow::anyhow!("boom")));
        assert_eq!(out, Err("boom".to_string()));
    }
}
