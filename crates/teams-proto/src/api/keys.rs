//! Team key wrap types.

use crate::wire::{Bytes, Nonce12, PublicKey32, Signature64};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
