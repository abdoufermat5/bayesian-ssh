//! Team, membership and agent types.

use super::accounts::{DeviceInfo, NewDevice};
use super::keys::WrappedKey;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Invites and removes members, wraps team keys, reads the audit log.
    Admin,
    /// Reads and writes the vault, uploads its own audit events.
    Member,
}

/// `POST /v1/teams`. The creator becomes the first admin. `initial_key` is
/// the team key (version 1) wrapped to the creator's own device and signed by
/// it; the team has no name on the server (names live inside the vault).
///
/// `org_id`: the organization the team belongs to; the caller must own it.
/// Without it the team goes to the oldest organization the caller owns, or to
/// a new one when they own none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTeamRequest {
    pub initial_key: WrappedKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
}

/// `GET /v1/teams` returns `Vec<TeamInfo>`; `GET /v1/teams/{team_id}` one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInfo {
    pub team_id: Uuid,
    /// The seat limit invites are checked against (the organization's, when
    /// the server has billing).
    pub seats: u32,
    pub member_count: u32,
    pub current_key_version: u32,
    /// True after a member is removed until an admin posts a new key version.
    pub rotation_required: bool,
    pub my_role: Role,
    pub created_at: DateTime<Utc>,
    /// `None` from a server without organizations.
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

/// Who a team member is. Agents are unattended identities created by an
/// admin (`POST /v1/teams/{team_id}/agents`); they take a seat like a human
/// member, are never admins, and can read the vault but not write to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    #[default]
    Human,
    Agent,
}

/// `GET /v1/teams/{team_id}/members` returns `Vec<Member>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub member_id: Uuid,
    pub account_id: Uuid,
    pub email: String,
    pub role: Role,
    /// `Human` from a server without agents.
    #[serde(default)]
    pub kind: MemberKind,
    pub devices: Vec<DeviceInfo>,
    pub joined_at: DateTime<Utc>,
}

/// `POST /v1/teams/{team_id}/invites` (admin). Fails with `SeatLimit` when
/// members plus pending invites would exceed `seats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateInviteRequest {
    pub email: String,
    pub role: Role,
}

/// `token` is returned once and never again. When the server can send mail it
/// also mails the token to the invitee (`emailed`); otherwise the admin sends
/// it out of band. Either way the admin gives their own device's Ed25519
/// fingerprint out of band, never through the server: the invitee pins it as
/// the team's trust root.
///
/// `GET /v1/teams/{team_id}/invites` (admin) returns the pending, unexpired
/// invites as `Vec<Invite>`, all with `token: None`.
/// `DELETE /v1/teams/{team_id}/invites/{invite_id}` (admin) revokes a pending
/// invite: no body, 204, and its seat is free again. An accepted or unknown
/// invite is `NotFound`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub invite_id: Uuid,
    pub team_id: Uuid,
    pub email: String,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
    pub token: Option<String>,
    /// The server mailed the token to `email` (only set on creation).
    #[serde(default)]
    pub emailed: bool,
}

/// `POST /v1/invites/accept`. The caller's device joins with no team key
/// yet; an admin device then wraps the current key to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptInviteRequest {
    pub token: String,
}

/// `DELETE /v1/teams/{team_id}/members/{member_id}` (admin) has no body,
/// returns 204, and sets `rotation_required` on the team.
///
/// `GET /v1/teams/{team_id}/devices/pending` (admin) lists member devices
/// that have no wrapped key for the current key version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDevices {
    pub key_version: u32,
    pub devices: Vec<DeviceInfo>,
}

/// `POST /v1/teams/{team_id}/agents` (admin): register an agent identity. The
/// admin generated the agent's device keys locally and sends only the public
/// halves. The agent becomes a member of kind [`MemberKind::Agent`] and takes
/// a seat (`SeatLimit` applies). The admin then wraps the team key to the
/// agent's device with `POST /keys`, like for any new device.
/// Removing an agent is `DELETE /v1/teams/{team_id}/members/{member_id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentRequest {
    /// Shown to admins; unique within the team (case-insensitive).
    pub name: String,
    pub device: NewDevice,
}

/// Response of `POST …/agents`; `GET /v1/teams/{team_id}/agents` (any member)
/// returns `Vec<AgentInfo>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub member_id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub device: DeviceInfo,
    pub created_at: DateTime<Utc>,
}
