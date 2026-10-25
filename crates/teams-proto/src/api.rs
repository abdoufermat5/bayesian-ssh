//! Request and response bodies, one section per endpoint group. Paths are
//! listed on each type; all are under [`crate::API_PREFIX`] and, except
//! account creation and auth, need `Authorization: Bearer <access_token>`.

use crate::wire::{Bytes, Fixed, Hash32, Nonce12, PublicKey32, Signature64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Body of every non-2xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 400
    InvalidRequest,
    /// 401: missing, expired or unknown token.
    Unauthorized,
    /// 403: authenticated, but not allowed (role, removed member, revoked device).
    Forbidden,
    /// 404
    NotFound,
    /// 409: optimistic-concurrency or uniqueness conflict.
    Conflict,
    /// 402: the team has no free seat for this invite.
    SeatLimit,
    /// 422: a signature or key failed verification.
    InvalidSignature,
    /// 409: a push used an old `key_version` after a member was removed;
    /// an admin must post a new key version first.
    RotationRequired,
    /// 409: an audit batch doesn't continue the device's hash chain.
    AuditChainBroken,
    /// 426
    ClientTooOld,
    /// 429
    RateLimited,
    /// 500
    Internal,
    /// 402: the team is read-only. It has more members than the free seats
    /// and no active subscription: pulls work, changes don't.
    PaymentRequired,
    /// 403: the server needs proof that the caller owns `email`: request a
    /// code with `POST /v1/accounts/email-code` and resend with `email_code`.
    EmailVerificationRequired,
    /// 403: the `email_code` is wrong, expired, already used, or was tried
    /// too often. Request a new one.
    InvalidEmailCode,
    /// 403: the team's audit storage is full. The upload stored nothing: the
    /// client keeps its events queued and tells the user. An admin raises the
    /// limit on the server (or the operator does); the next upload goes through.
    AuditQuotaExceeded,
    /// A code this client doesn't know (sent by a newer server).
    #[serde(other)]
    Unknown,
}

// ── Accounts, devices, auth ──────────────────────────────────────────────────

/// A device's public keys. Private keys never leave the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewDevice {
    /// Human label, e.g. "laptop". Visible to the server and team admins.
    pub name: String,
    /// Key agreement key, used to receive wrapped team keys.
    pub x25519_public: PublicKey32,
    /// Signing key for entries, audit events, key wraps and auth challenges.
    pub ed25519_public: PublicKey32,
}

/// `POST /v1/accounts` (unauthenticated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAccountRequest {
    pub email: String,
    pub device: NewDevice,
    /// A pending invite for `email`. Servers that only accept invited
    /// accounts check it here; it is not consumed (`/invites/accept` does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_token: Option<String>,
    /// The code mailed by `POST /v1/accounts/email-code`, when the server
    /// answered `EmailVerificationRequired`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_code: Option<String>,
}

/// `POST /v1/accounts/email-code` (unauthenticated): mail a one-time code to
/// `email`, proving ownership for [`CreateAccountRequest::email_code`].
///
/// The answer is the same whether or not a code was sent (unknown, already
/// registered or not allowed to sign up), so it can't be used to probe which
/// addresses exist. Codes are single use and short lived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailCodeRequest {
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailCodeResponse {
    /// How long a code stays valid.
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAccountResponse {
    pub account_id: Uuid,
    pub device_id: Uuid,
}

/// `POST /v1/devices` (`AddDeviceRequest`, `AddDeviceResponse`): add another
/// device to the caller's account. The server does **not** serve it: the types
/// are kept for wire compatibility only. A bearer token alone must not be able
/// to add a device, or a stolen one-hour token would become permanent access.
///
/// `DELETE /v1/devices/{device_id}` revokes a device: no body, 204. Allowed
/// for the device's own account and for an active admin of a team in which the
/// device's account is an active member. Accounts have one device each, so an
/// owner revokes the device they are signed in on; a lost or stolen laptop is
/// revoked by a team admin. Revocation is permanent: `revoked_at` is set, the
/// device's tokens stop working at once, and every team where the account is
/// an active member gets `rotation_required`, so the next key version leaves
/// the device out. The device gets `Forbidden` everywhere. It stays in member
/// device lists, with `revoked_at` set ([`DeviceInfo::revoked_at`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddDeviceRequest {
    pub device: NewDevice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddDeviceResponse {
    pub device_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub x25519_public: PublicKey32,
    pub ed25519_public: PublicKey32,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// `POST /v1/auth/challenge` (unauthenticated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeRequest {
    pub device_id: Uuid,
}

/// Single use; expires after 60 seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub challenge_id: Uuid,
    pub nonce: Fixed<32>,
    pub expires_at: DateTime<Utc>,
}

/// `POST /v1/auth/token` (unauthenticated). `signature` is the device's
/// Ed25519 signature over [`crate::layout::auth_signing_bytes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRequest {
    pub challenge_id: Uuid,
    pub device_id: Uuid,
    pub signature: Signature64,
}

/// Opaque bearer token, valid for one hour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

// ── Teams and members ────────────────────────────────────────────────────────

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

// ── Organizations ────────────────────────────────────────────────────────────
//
// An organization owns teams, and with billing, the subscription and seats:
// a person in several of its teams takes one seat. Like teams, it has no name
// on the server; clients name it after its owners.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    /// Manages billing and owners, creates teams in the organization.
    Owner,
    /// Belongs to at least one of its teams.
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgOwner {
    pub account_id: Uuid,
    pub email: String,
}

/// `GET /v1/orgs` returns `Vec<OrgInfo>`: the organizations the caller owns or
/// has a team in. `GET /v1/orgs/{org_id}` one.
/// `POST /v1/orgs` (no body) creates an empty one owned by the caller; refused
/// with `PaymentRequired` while they own another without a subscription (one
/// free organization per account).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgInfo {
    pub org_id: Uuid,
    pub my_role: OrgRole,
    pub owners: Vec<OrgOwner>,
    /// Owners see every team; members only theirs.
    pub team_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// `POST /v1/orgs/{org_id}/owners` (owner): the account must be an active
/// member of one of the organization's teams. Returns the updated `OrgInfo`.
/// `DELETE /v1/orgs/{org_id}/owners/{account_id}` (owner) returns 204; the
/// last owner can't be removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddOwnerRequest {
    pub email: String,
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

// ── Team keys ────────────────────────────────────────────────────────────────

/// A team key wrapped to one device (ephemeral-static X25519, HKDF-SHA256,
/// AES-256-GCM; see `PROTOCOL.md`) and signed by the admin device that
/// wrapped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    pub team_id: Uuid,
    pub key_version: u32,
    pub recipient_device_id: Uuid,
    pub wrapping_device_id: Uuid,
    pub ephemeral_public: PublicKey32,
    pub nonce: Nonce12,
    /// AES-256-GCM of the 32-byte team key, tag appended.
    pub ciphertext: Bytes,
    /// Over [`crate::layout::key_wrap_signing_bytes`].
    pub signature: Signature64,
}

/// `POST /v1/teams/{team_id}/keys` (admin).
///
/// Same `key_version` as current: grants the key to pending devices.
/// `current + 1`: a rotation, which must cover every active device of every
/// remaining member; it becomes current and clears `rotation_required`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutKeysRequest {
    pub keys: Vec<WrappedKey>,
}

// `GET /v1/teams/{team_id}/keys` returns `Vec<WrappedKey>` addressed to the
// calling device, every version it holds, oldest first.

// ── Vault ────────────────────────────────────────────────────────────────────

/// One encrypted vault entry. The server stores every revision (append-only)
/// and never sees the plaintext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedEntry {
    /// Client-chosen UUIDv4, stable across edits.
    pub entry_id: Uuid,
    /// Assigned by the server, team-wide and strictly increasing. Send 0.
    pub revision: u64,
    /// The `revision` this edit was based on; 0 for a new entry.
    pub base_revision: u64,
    pub key_version: u32,
    pub author_device_id: Uuid,
    pub nonce: Nonce12,
    /// AES-256-GCM of a JSON [`VaultPayload`], tag appended, AAD =
    /// [`crate::layout::entry_aad`].
    pub ciphertext: Bytes,
    /// Over [`crate::layout::entry_signing_bytes`].
    pub signature: Signature64,
}

/// `GET /v1/teams/{team_id}/vault?since=<revision>&limit=<n>`: entries with
/// `revision > since`, ascending; `limit` defaults to 500, max 1000.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullResponse {
    pub entries: Vec<EncryptedEntry>,
    pub latest_revision: u64,
    pub has_more: bool,
}

/// `POST /v1/teams/{team_id}/vault`. Each entry is accepted only if its
/// `base_revision` equals the entry's current revision (0 if new);
/// otherwise it's reported in `conflicts` and nothing is stored for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRequest {
    pub entries: Vec<EncryptedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResponse {
    pub accepted: Vec<Accepted>,
    pub conflicts: Vec<PushConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accepted {
    pub entry_id: Uuid,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushConflict {
    pub entry_id: Uuid,
    pub current_revision: u64,
}

/// Plaintext inside [`EncryptedEntry::ciphertext`]. Only clients see this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VaultPayload {
    Connection(ConnectionRecord),
    /// The team's display name and settings. At most one live entry.
    TeamProfile {
        name: String,
    },
    /// Deletion marker; the entry stays in history.
    Tombstone,
    /// An agent's policy, signed by an admin device. Clients verify the
    /// signature against the pinned admin key before using it, so the server
    /// can't widen it.
    AgentPolicy(SignedAgentPolicy),
}

/// An admin-signed agent policy (bssh AI, Teams). `policy_json` is the exact
/// text that was signed (a serialized `ai` policy); it is parsed only after
/// the signature verifies. The server sees none of this: it lives in the
/// encrypted vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAgentPolicy {
    /// The agent's device id the policy applies to.
    pub agent_device_id: Uuid,
    /// Policy name, e.g. `default`.
    pub name: String,
    pub policy_json: String,
    /// Bumped on every re-sign; a gateway refuses to go back to a lower one.
    pub version: u64,
    pub signed_at: DateTime<Utc>,
    pub signer_device_id: Uuid,
    /// Over [`crate::layout::policy_signing_bytes`].
    pub signature: Signature64,
}

/// A shared connection. Deliberately has no key paths or passwords: those are
/// per-user and stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    #[serde(default)]
    pub bastion: Option<String>,
    #[serde(default)]
    pub bastion_user: Option<String>,
    #[serde(default)]
    pub use_kerberos: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

// ── Billing ──────────────────────────────────────────────────────────────────
//
// Payment happens on the billing provider's pages (checkout and portal): the
// server only hands out their URLs, and learns the outcome from the provider.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Up to `free_seats` members, no subscription.
    Free,
    /// A paid subscription for `paid_seats` seats.
    Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingInterval {
    Month,
    Year,
}

/// `GET /v1/orgs/{org_id}/billing` (any member of the organization), or
/// `GET /v1/teams/{team_id}/billing` (any team member) for the team's
/// organization. With billing, seats are counted across the organization:
/// distinct members of its teams plus pending invites to new people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingInfo {
    /// False on a server without billing (self-hosted): `seat_limit` is fixed.
    pub billing_enabled: bool,
    pub plan: Plan,
    pub free_seats: u32,
    /// Seats of the subscription; 0 without one.
    pub paid_seats: u32,
    /// What invites are checked against: members plus pending invites.
    pub seat_limit: u32,
    pub seats_used: u32,
    /// The provider's subscription status (`active`, `past_due`, `canceled`…).
    pub status: Option<String>,
    pub interval: Option<BillingInterval>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    /// More members than the free seats and no active subscription: pushes
    /// and invites fail with `PaymentRequired`/`SeatLimit` until paid.
    pub read_only: bool,
    /// The organization this billing belongs to; `None` from a server without
    /// organizations.
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

/// `POST /v1/orgs/{org_id}/billing/checkout` (owner): start a subscription.
/// `POST /v1/teams/{team_id}/billing/checkout` does the same for the team's
/// organization; the caller must own it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutRequest {
    /// At least the seats in use (members plus pending invites).
    pub seats: u32,
    pub interval: BillingInterval,
}

/// Response of `POST …/billing/checkout` and `POST …/billing/portal` (admin):
/// a page of the billing provider to open in a browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingRedirect {
    pub url: String,
}

// ── Agents ───────────────────────────────────────────────────────────────────

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

// ── Approval relay ───────────────────────────────────────────────────────────
//
// An agent that needs a human's approval posts a `Request` encrypted to the
// admins' devices; an admin answers with a `Decision` encrypted to the
// requesting device (and the other admins). The server queues and expires the
// messages; it sees the team, the sender device, the size and the expiry, and
// nothing of what is asked or decided.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayKind {
    Request,
    Decision,
}

impl RelayKind {
    pub fn tag(self) -> u8 {
        match self {
            RelayKind::Request => 1,
            RelayKind::Decision => 2,
        }
    }
}

/// The content key of a [`RelayMessage`], wrapped to one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayRecipient {
    pub device_id: Uuid,
    pub ephemeral_public: PublicKey32,
    pub nonce: Nonce12,
    /// AES-256-GCM of the 32-byte content key, tag appended.
    pub wrapped_key: Bytes,
}

/// An end-to-end encrypted, signed message between devices of one team.
///
/// `POST /v1/teams/{team_id}/relay` (any active member for a `Request`, admin
/// for a `Decision`). The server checks the signature against the sender
/// device's key, that the sender is the caller, that recipients are active
/// devices of the team (admins for a `Request`), the size (64 KiB) and that
/// `expires_at` is in the future and at most one hour away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayMessage {
    /// Client-chosen UUIDv4; makes posts idempotent.
    pub message_id: Uuid,
    pub team_id: Uuid,
    /// Correlates a decision with its request; opaque to the server.
    pub request_id: Uuid,
    pub kind: RelayKind,
    pub sender_device_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub nonce: Nonce12,
    /// AES-256-GCM of a JSON payload under the content key, AAD =
    /// [`crate::layout::relay_aad`].
    pub ciphertext: Bytes,
    pub recipients: Vec<RelayRecipient>,
    /// Over [`crate::layout::relay_signing_bytes`].
    pub signature: Signature64,
}

/// Response of `POST …/relay`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayAck {
    pub message_id: Uuid,
    /// The queue position; pass it as `after` to skip past this message.
    pub seq: u64,
}

/// `GET /v1/teams/{team_id}/relay?kind=&request_id=&after=&wait=`: unexpired
/// messages addressed to the calling device with `seq > after`, ascending.
/// With nothing to return, waits up to `wait` seconds (default 0, max 30) for
/// a message to arrive, then answers with an empty list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayPoll {
    pub messages: Vec<RelayEnvelope>,
    /// Pass as `after` next time.
    pub next: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayEnvelope {
    /// Server-assigned, strictly increasing per server.
    pub seq: u64,
    pub message: RelayMessage,
}

// ── Audit ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    SessionStart,
    SessionEnd,
    VaultRead,
    VaultWrite,
    /// bssh AI: a tool call and its policy decision. Detail is encrypted.
    AiCall,
    /// bssh AI: an approval request and its outcome.
    AiApproval,
    /// bssh AI: a change plan applied or undone.
    AiChange,
    /// bssh AI: run started or ended, circuit breaker tripped.
    AiRun,
}

impl AuditKind {
    /// Byte used in [`crate::layout::audit_signing_bytes`]. Never reuse a tag.
    pub fn tag(self) -> u8 {
        match self {
            AuditKind::SessionStart => 1,
            AuditKind::SessionEnd => 2,
            AuditKind::VaultRead => 3,
            AuditKind::VaultWrite => 4,
            AuditKind::AiCall => 5,
            AuditKind::AiApproval => 6,
            AuditKind::AiChange => 7,
            AuditKind::AiRun => 8,
        }
    }

    /// Kinds 1 to 4 predate bssh AI and are signed with the v1 layout (when
    /// they carry no actor or detail).
    pub fn is_ai(self) -> bool {
        self.tag() >= 5
    }

    /// Wire name, as used in `kind` query filters and server storage.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::SessionStart => "session_start",
            AuditKind::SessionEnd => "session_end",
            AuditKind::VaultRead => "vault_read",
            AuditKind::VaultWrite => "vault_write",
            AuditKind::AiCall => "ai_call",
            AuditKind::AiApproval => "ai_approval",
            AuditKind::AiChange => "ai_change",
            AuditKind::AiRun => "ai_run",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "session_start" => AuditKind::SessionStart,
            "session_end" => AuditKind::SessionEnd,
            "vault_read" => AuditKind::VaultRead,
            "vault_write" => AuditKind::VaultWrite,
            "ai_call" => AuditKind::AiCall,
            "ai_approval" => AuditKind::AiApproval,
            "ai_change" => AuditKind::AiChange,
            "ai_run" => AuditKind::AiRun,
            _ => return None,
        })
    }
}

/// Who acted. Signed with the event (v2 layout), visible to the server so
/// "which agent did something this week" can be queried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub kind: MemberKind,
    /// Agent name, or the member's email for a human.
    pub name: String,
    /// The human an interactive agent acts for ("Claude Code on behalf of alice").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
}

/// Encrypted detail of an AI event (commands, hosts, outcome). AES-256-GCM
/// under the team's audit key, see `PROTOCOL.md`. Opaque to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditDetail {
    pub key_version: u32,
    pub nonce: Nonce12,
    pub ciphertext: Bytes,
}

/// A client-signed audit event. Events per device form a hash chain
/// (`seq`, `prev_hash`), see [`crate::layout::audit_event_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Client-generated, makes uploads idempotent.
    pub event_id: Uuid,
    pub team_id: Uuid,
    pub device_id: Uuid,
    /// Starts at 0 per device, no gaps.
    pub seq: u64,
    pub kind: AuditKind,
    /// The vault entry involved, if any (opaque to the server).
    pub entry_id: Option<Uuid>,
    /// Millisecond precision is all that's signed.
    pub occurred_at: DateTime<Utc>,
    pub prev_hash: Hash32,
    pub signature: Signature64,
    /// Who acted; set on AI events. Absent on kinds 1 to 4 from older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<AuditActor>,
    /// Encrypted detail; set on AI events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<AuditDetail>,
}

/// `POST /v1/teams/{team_id}/audit`: events from the calling device, in
/// `seq` order. Already-stored `event_id`s are reported as duplicates, not
/// errors, so retries are safe. A batch that doesn't continue the stored
/// chain fails with `AuditChainBroken` and stores nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestAuditRequest {
    pub events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestAuditResponse {
    pub accepted: Vec<Uuid>,
    pub duplicates: Vec<Uuid>,
}

/// Sort order of `GET /v1/teams/{team_id}/audit`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOrder {
    /// `received_at` descending, then `device_id` descending, then `seq`
    /// descending: the newest events first. The default.
    #[default]
    Newest,
    /// `device_id` ascending, then `seq` ascending: each device's hash chain
    /// in order, one device after the other. For verification and export.
    Chain,
}

/// Query string of `GET /v1/teams/{team_id}/audit` (admin). The response is
/// `Vec<AuditRecord>` in the requested [`AuditOrder`].
///
/// Filters: `device_id`, `entry_id`, `kind` (an [`AuditKind::as_str`] name),
/// `actor` (an exact actor name), `since` (`received_at >= since`: the time the
/// server observed the event, not the signed `occurred_at`). `limit` is clamped
/// by the server to 1 to 1000 (not rejected), default 100.
///
/// Paging is keyset-based: ask for a page, then ask again with the cursor of
/// the last record ([`AuditListQuery::after`]) until a page holds fewer records
/// than the effective (clamped) `limit`, which is the last. The cursor is stable
/// while events keep arriving: new events never shift or repeat the pages
/// already read.
///
/// - `order=newest` (default): cursor `before_received_at`, `before_device_id`
///   and `before_seq`, all three or none. Returns the events strictly after
///   that position in the newest-first order. All filters apply.
/// - `order=chain`: cursor `after_device_id` and `after_seq`, both or none.
///   Returns the events strictly after that position in the chain order. Only
///   `device_id` and `limit` are allowed besides the cursor: a filter would
///   drop events from the middle of a chain and break the contiguity that
///   verification relies on.
///
/// Anything else is `InvalidRequest`: a partial cursor, a cursor of the other
/// order, or a filter with `order=chain`.
///
/// A server older than paging ignores the cursor and the order and answers
/// the first page again. A client that may talk to one must stop as soon as a
/// page does not advance past its cursor (the last record is the same as the
/// previous page's, or the page is not past the cursor in the requested
/// order), instead of looping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AuditKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Absent means [`AuditOrder::Newest`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<AuditOrder>,
    /// Newest cursor, with `before_device_id` and `before_seq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_received_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_seq: Option<u64>,
    /// Chain cursor, with `after_seq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

impl AuditListQuery {
    /// The query for the page after the one that ended with `last`: the
    /// cursor of the query's order is set from `last` (the other order's
    /// cursor is cleared), every other field is kept.
    pub fn after(&self, last: &AuditRecord) -> AuditListQuery {
        let mut next = self.clone();
        match self.order.unwrap_or_default() {
            AuditOrder::Newest => {
                next.before_received_at = Some(last.received_at);
                next.before_device_id = Some(last.event.device_id);
                next.before_seq = Some(last.event.seq);
                next.after_device_id = None;
                next.after_seq = None;
            }
            AuditOrder::Chain => {
                next.after_device_id = Some(last.event.device_id);
                next.after_seq = Some(last.event.seq);
                next.before_received_at = None;
                next.before_device_id = None;
                next.before_seq = None;
            }
        }
        next
    }
}

/// One stored audit event as `GET …/audit` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub event: AuditEvent,
    /// Observed by the server, not signed.
    pub received_at: DateTime<Utc>,
    pub client_ip: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_snake_case() {
        let e = ApiError {
            code: ErrorCode::RotationRequired,
            message: "x".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"rotation_required\""), "{json}");
    }

    #[test]
    fn vault_payload_is_tagged() {
        let p = VaultPayload::Tombstone;
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"type":"tombstone"}"#
        );
        let c: VaultPayload = serde_json::from_str(
            r#"{"type":"connection","name":"db","host":"10.0.0.5","user":"ops","port":22}"#,
        )
        .unwrap();
        assert!(matches!(c, VaultPayload::Connection(ref r) if r.tags.is_empty()));
    }

    #[test]
    fn invite_token_is_optional_on_the_wire() {
        let req = CreateAccountRequest {
            email: "ana@example.com".into(),
            device: NewDevice {
                name: "laptop".into(),
                x25519_public: Fixed::ZERO,
                ed25519_public: Fixed::ZERO,
            },
            invite_token: None,
            email_code: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("invite_token"), "{json}");
        assert!(!json.contains("email_code"), "{json}");
        // A request from an older client still parses.
        let old: CreateAccountRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(old.invite_token, None);
        assert_eq!(old.email_code, None);
    }

    #[test]
    fn invites_from_older_servers_are_not_emailed() {
        let json = r#"{"invite_id":"00000000-0000-0000-0000-000000000001",
            "team_id":"00000000-0000-0000-0000-000000000002","email":"ana@example.com",
            "role":"member","expires_at":"2026-10-25T00:00:00Z","token":"t"}"#;
        let invite: Invite = serde_json::from_str(json).unwrap();
        assert!(!invite.emailed);
    }

    #[test]
    fn email_verification_codes_round_trip() {
        for code in [
            ErrorCode::EmailVerificationRequired,
            ErrorCode::InvalidEmailCode,
        ] {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
        }
    }

    #[test]
    fn unknown_error_codes_still_parse() {
        let e: ApiError =
            serde_json::from_str(r#"{"code":"something_new","message":"x"}"#).unwrap();
        assert_eq!(e.code, ErrorCode::Unknown);
        let e: ApiError =
            serde_json::from_str(r#"{"code":"payment_required","message":"x"}"#).unwrap();
        assert_eq!(e.code, ErrorCode::PaymentRequired);
    }

    #[test]
    fn audit_quota_code_round_trips() {
        let json = serde_json::to_string(&ErrorCode::AuditQuotaExceeded).unwrap();
        assert_eq!(json, "\"audit_quota_exceeded\"");
        assert_eq!(
            serde_json::from_str::<ErrorCode>(&json).unwrap(),
            ErrorCode::AuditQuotaExceeded
        );
    }

    fn record(device: u8, seq: u64, received_secs: i64) -> AuditRecord {
        AuditRecord {
            event: AuditEvent {
                event_id: Uuid::from_bytes([9; 16]),
                team_id: Uuid::from_bytes([1; 16]),
                device_id: Uuid::from_bytes([device; 16]),
                seq,
                kind: AuditKind::SessionStart,
                entry_id: None,
                occurred_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
                prev_hash: Fixed::ZERO,
                signature: Fixed::ZERO,
                actor: None,
                detail: None,
            },
            received_at: chrono::DateTime::from_timestamp(received_secs, 0).unwrap(),
            client_ip: None,
        }
    }

    #[test]
    fn audit_order_is_lowercase_and_defaults_to_newest() {
        assert_eq!(
            serde_json::to_string(&AuditOrder::Chain).unwrap(),
            "\"chain\""
        );
        assert_eq!(
            serde_json::from_str::<AuditOrder>("\"newest\"").unwrap(),
            AuditOrder::Newest
        );
        assert_eq!(AuditOrder::default(), AuditOrder::Newest);
    }

    #[test]
    fn empty_audit_query_serializes_to_nothing() {
        let json = serde_json::to_string(&AuditListQuery::default()).unwrap();
        assert_eq!(json, "{}");
        let q: AuditListQuery = serde_json::from_str(r#"{"kind":"vault_read","limit":5}"#).unwrap();
        assert_eq!(q.kind, Some(AuditKind::VaultRead));
        assert_eq!(q.limit, Some(5));
    }

    #[test]
    fn after_sets_the_newest_cursor_and_keeps_filters() {
        let q = AuditListQuery {
            actor: Some("ci".into()),
            limit: Some(50),
            after_device_id: Some(Uuid::nil()),
            after_seq: Some(3),
            ..Default::default()
        };
        let next = q.after(&record(4, 7, 1_700_000_500));
        assert_eq!(next.actor.as_deref(), Some("ci"));
        assert_eq!(next.limit, Some(50));
        assert_eq!(next.before_device_id, Some(Uuid::from_bytes([4; 16])));
        assert_eq!(next.before_seq, Some(7));
        assert_eq!(
            next.before_received_at,
            chrono::DateTime::from_timestamp(1_700_000_500, 0)
        );
        assert_eq!((next.after_device_id, next.after_seq), (None, None));
    }

    #[test]
    fn after_sets_the_chain_cursor_in_chain_order() {
        let q = AuditListQuery {
            order: Some(AuditOrder::Chain),
            device_id: Some(Uuid::from_bytes([4; 16])),
            limit: Some(1000),
            before_seq: Some(1),
            ..Default::default()
        };
        let next = q.after(&record(4, 7, 1_700_000_500));
        assert_eq!(next.order, Some(AuditOrder::Chain));
        assert_eq!(next.device_id, Some(Uuid::from_bytes([4; 16])));
        assert_eq!(next.after_device_id, Some(Uuid::from_bytes([4; 16])));
        assert_eq!(next.after_seq, Some(7));
        assert_eq!(
            (
                next.before_received_at,
                next.before_device_id,
                next.before_seq
            ),
            (None, None, None)
        );
    }
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    // An audit event as written by a pre-AI client (kinds 1-4, no actor/detail).
    const OLD_EVENT: &str = r#"{
        "event_id":"06060606-0606-0606-0606-060606060606",
        "team_id":"01010101-0101-0101-0101-010101010101",
        "device_id":"04040404-0404-0404-0404-040404040404",
        "seq":7,"kind":"session_start",
        "entry_id":"02020202-0202-0202-0202-020202020202",
        "occurred_at":"2023-11-14T22:13:20.123Z",
        "prev_hash":"CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=",
        "signature":"rxfVq0KLe2evDDwJi3+L3wfJ3elaQEOv9jRqOaTt2WecUc7YuwjcupTSvqKYLo77/Cn4zekMuwpOuOe+Zdm5Cg=="
    }"#;

    #[test]
    fn old_audit_events_decode_and_reserialize_unchanged() {
        let ev: AuditEvent = serde_json::from_str(OLD_EVENT).unwrap();
        assert!(ev.actor.is_none() && ev.detail.is_none());
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            !json.contains("actor") && !json.contains("detail"),
            "{json}"
        );
        let again: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, again);
    }

    #[test]
    fn old_tags_are_never_reused() {
        assert_eq!(
            [
                AuditKind::SessionStart,
                AuditKind::SessionEnd,
                AuditKind::VaultRead,
                AuditKind::VaultWrite
            ]
            .map(AuditKind::tag),
            [1, 2, 3, 4]
        );
        assert_eq!(
            [
                AuditKind::AiCall,
                AuditKind::AiApproval,
                AuditKind::AiChange,
                AuditKind::AiRun
            ]
            .map(AuditKind::tag),
            [5, 6, 7, 8]
        );
        for k in [
            AuditKind::SessionStart,
            AuditKind::AiCall,
            AuditKind::AiApproval,
            AuditKind::AiChange,
            AuditKind::AiRun,
        ] {
            assert_eq!(AuditKind::parse(k.as_str()), Some(k));
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.as_str()));
        }
    }

    #[test]
    fn old_vault_payloads_and_members_still_decode() {
        let p: VaultPayload =
            serde_json::from_str(r#"{"type":"team_profile","name":"ops"}"#).unwrap();
        assert!(matches!(p, VaultPayload::TeamProfile { .. }));
        let m: Member = serde_json::from_str(
            r#"{"member_id":"01010101-0101-0101-0101-010101010101",
                "account_id":"01010101-0101-0101-0101-010101010101",
                "email":"a@b.c","role":"admin","devices":[],
                "joined_at":"2023-11-14T22:13:20Z"}"#,
        )
        .unwrap();
        assert_eq!(m.kind, MemberKind::Human);
    }

    #[test]
    fn agent_policy_payload_is_tagged() {
        let p = VaultPayload::AgentPolicy(SignedAgentPolicy {
            agent_device_id: Uuid::nil(),
            name: "default".into(),
            policy_json: "{}".into(),
            version: 1,
            signed_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            signer_device_id: Uuid::nil(),
            signature: Fixed::ZERO,
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.starts_with(r#"{"type":"agent_policy""#), "{json}");
        assert_eq!(serde_json::from_str::<VaultPayload>(&json).unwrap(), p);
    }
}
