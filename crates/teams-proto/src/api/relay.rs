//! Approval relay types.
//!
//! An agent that needs a human's approval posts a `Request` encrypted to the
//! admins' devices; an admin answers with a `Decision` encrypted to the
//! requesting device (and the other admins). The server queues and expires the
//! messages; it sees the team, the sender device, the size and the expiry, and
//! nothing of what is asked or decided.

use crate::wire::{Bytes, Nonce12, PublicKey32, Signature64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
