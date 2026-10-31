//! Encrypted vault entry types and the plaintext payloads they carry.

use crate::wire::{Bytes, Nonce12, Signature64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
