//! Wire shapes returned to the frontend by the Teams commands.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use bayesian_ssh::services::teams::session::{PreviewDevice, TeamSync};
use bayesian_ssh::services::teams::sync::AUDIT_QUOTA_MESSAGE;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct EmailCodeDto {
    pub(super) expires_in_secs: u64,
}

// ── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IdentityDto {
    pub(super) email: String,
    pub(super) server_url: String,
    pub(super) device_id: String,
}

#[derive(Serialize)]
pub struct TeamDto {
    pub(super) team_id: String,
    /// The team's inventory source; its connections carry this `source_id`.
    pub(super) source_id: String,
    pub(super) name: String,
    pub(super) hosts: usize,
    pub(super) last_sync_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct TeamsStatus {
    pub(super) identity: Option<IdentityDto>,
    pub(super) unlocked: bool,
    pub(super) fingerprint: Option<String>,
    pub(super) teams: Vec<TeamDto>,
}

#[derive(Serialize)]
pub struct TeamSyncDto {
    pub(super) team_id: String,
    pub(super) name: Option<String>,
    pub(super) waiting_for_key: bool,
    pub(super) pulled: usize,
    pub(super) pushed: usize,
    pub(super) conflicts_resolved: usize,
    pub(super) audit_uploaded: usize,
    pub(super) dropped_edits: usize,
    pub(super) rejected: Vec<String>,
    pub(super) unreadable: usize,
    pub(super) rotated_to: Option<u32>,
    pub(super) rotation_required: bool,
    pub(super) errors: Vec<String>,
    pub(super) hosts_added: usize,
    pub(super) hosts_updated: usize,
    pub(super) hosts_stale: usize,
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
            errors: r
                .errors
                .iter()
                .cloned()
                .chain(
                    r.audit_quota_exceeded
                        .then(|| AUDIT_QUOTA_MESSAGE.to_string()),
                )
                .collect(),
            hosts_added: s.hosts.as_ref().map_or(0, |h| h.added),
            hosts_updated: s.hosts.as_ref().map_or(0, |h| h.updated),
            hosts_stale: s.hosts.as_ref().map_or(0, |h| h.stale),
        }
    }
}

#[derive(Serialize)]
pub struct TeamSyncResult {
    pub(super) team: String,
    pub(super) result: Option<TeamSyncDto>,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
pub struct InviteDto {
    pub(super) email: String,
    pub(super) expires_at: DateTime<Utc>,
    pub(super) token: String,
    pub(super) admin_fingerprint: String,
    pub(super) server_url: String,
    /// The server mailed the token to the invitee; it is still returned as a
    /// fallback. The admin fingerprint is never mailed by the server.
    pub(super) emailed: bool,
}

#[derive(Serialize)]
pub struct JoinDto {
    pub(super) team_id: String,
    pub(super) name: String,
    pub(super) my_fingerprint: String,
}

#[derive(Serialize)]
pub struct DeviceDto {
    pub(super) name: String,
    pub(super) fingerprint: String,
}

#[derive(Serialize)]
pub struct PendingDto {
    pub(super) email: String,
    pub(super) device: DeviceDto,
}

/// A pending invite as an admin sees it: no token.
#[derive(Serialize)]
pub struct PendingInviteDto {
    pub(super) email: String,
    pub(super) role: &'static str,
    pub(super) expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MemberDto {
    pub(super) email: String,
    pub(super) role: &'static str,
    pub(super) devices: Vec<DeviceDto>,
}

#[derive(Serialize)]
pub struct AuditLineDto {
    pub(super) occurred_at: DateTime<Utc>,
    pub(super) received_at: DateTime<Utc>,
    pub(super) kind: &'static str,
    pub(super) who: String,
    pub(super) device_name: String,
    pub(super) host: Option<String>,
    pub(super) client_ip: Option<String>,
    /// "valid", "bad_signature", "unknown_device" or "wrong_team".
    pub(super) check: &'static str,
}

#[derive(Serialize)]
pub struct TeamAttentionDto {
    pub(super) team_id: String,
    pub(super) role: &'static str,
    pub(super) pending_devices: usize,
    pub(super) waiting_for_key: bool,
    pub(super) removed: bool,
}

#[derive(Serialize)]
pub struct PreviewDeviceDto {
    pub(super) email: String,
    pub(super) name: String,
    pub(super) fingerprint: String,
    pub(super) approved: bool,
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
    pub(super) email: String,
    pub(super) removed_devices: Vec<PreviewDeviceDto>,
    pub(super) next_key_version: u32,
    pub(super) recipients: Vec<PreviewDeviceDto>,
    pub(super) blocking: Vec<PreviewDeviceDto>,
    pub(super) shared_hosts: usize,
}

#[derive(Serialize)]
pub struct TeamAuditDto {
    pub(super) lines: Vec<AuditLineDto>,
    pub(super) chains: usize,
    pub(super) verified_events: usize,
    pub(super) problems: Vec<String>,
    pub(super) invalid_events: usize,
    pub(super) complete: bool,
    pub(super) clean: bool,
}
