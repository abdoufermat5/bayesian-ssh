//! Response error body and the error code vocabulary.

use serde::{Deserialize, Serialize};

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
