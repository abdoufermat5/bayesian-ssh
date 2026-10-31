//! Account, device and authentication types.

use crate::wire::{Fixed, PublicKey32, Signature64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
