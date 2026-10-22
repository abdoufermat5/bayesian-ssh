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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAccountResponse {
    pub account_id: Uuid,
    pub device_id: Uuid,
}

/// `POST /v1/devices`: add another device to the caller's account.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTeamRequest {
    pub initial_key: WrappedKey,
}

/// `GET /v1/teams` returns `Vec<TeamInfo>`; `GET /v1/teams/{team_id}` one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInfo {
    pub team_id: Uuid,
    pub seats: u32,
    pub member_count: u32,
    pub current_key_version: u32,
    /// True after a member is removed until an admin posts a new key version.
    pub rotation_required: bool,
    pub my_role: Role,
    pub created_at: DateTime<Utc>,
}

/// `GET /v1/teams/{team_id}/members` returns `Vec<Member>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub member_id: Uuid,
    pub account_id: Uuid,
    pub email: String,
    pub role: Role,
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

/// `token` is returned once and never again; the admin sends it to the
/// invitee out of band together with their own device's Ed25519 fingerprint,
/// which the invitee pins as the team's trust root.
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

/// `GET /v1/teams/{team_id}/billing` (any member).
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
}

/// `POST /v1/teams/{team_id}/billing/checkout` (admin): start a subscription.
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

// ── Audit ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    SessionStart,
    SessionEnd,
    VaultRead,
    VaultWrite,
}

impl AuditKind {
    /// Byte used in [`crate::layout::audit_signing_bytes`]. Never reuse a tag.
    pub fn tag(self) -> u8 {
        match self {
            AuditKind::SessionStart => 1,
            AuditKind::SessionEnd => 2,
            AuditKind::VaultRead => 3,
            AuditKind::VaultWrite => 4,
        }
    }
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

/// `GET /v1/teams/{team_id}/audit?device_id=&entry_id=&since=&limit=` (admin)
/// returns `Vec<AuditRecord>`, newest first, `limit` default 100, max 1000.
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
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("invite_token"), "{json}");
        // A request from an older client still parses.
        let old: CreateAccountRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(old.invite_token, None);
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
}
