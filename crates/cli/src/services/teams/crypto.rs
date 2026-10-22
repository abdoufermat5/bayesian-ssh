//! Client-side cryptography for bssh Teams.
//!
//! Implements the recipes in `crates/teams-proto/PROTOCOL.md` exactly. Every
//! authenticated or signed byte string comes from `bssh_teams_proto::layout`;
//! never build one by hand here.

use bssh_teams_proto::api::{
    AuditEvent, DeviceInfo, EncryptedEntry, NewDevice, VaultPayload, WrappedKey,
};
use bssh_teams_proto::wire::{PublicKey32, Signature64};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum TeamsCryptoError {
    #[error("signature verification failed")]
    BadSignature,
    #[error("decryption failed (wrong key or tampered data)")]
    Decrypt,
    #[error("wrapped key was not signed by a trusted admin device")]
    UntrustedSigner,
    #[error("key agreement produced a weak shared secret")]
    WeakKey,
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("sealed device keys: {0}")]
    Sealed(String),
}

pub type Result<T> = std::result::Result<T, TeamsCryptoError>;

/// A device's two private keys (X25519 for receiving team keys, Ed25519 for
/// signing). Zeroized on drop; never serialized except via [`DeviceKeys::seal`].
pub struct DeviceKeys {
    // Implementation-defined fields.
}

impl DeviceKeys {
    /// Fresh keys from the OS RNG.
    pub fn generate() -> Self {
        todo!("teams crypto")
    }

    /// Public half, as registered with the server.
    pub fn to_new_device(&self, name: &str) -> NewDevice {
        let _ = name;
        todo!("teams crypto")
    }

    pub fn ed25519_public(&self) -> PublicKey32 {
        todo!("teams crypto")
    }

    pub fn x25519_public(&self) -> PublicKey32 {
        todo!("teams crypto")
    }

    /// Encrypt both private keys with `passphrase` using `services::crypto`
    /// (Argon2id + AES-256-GCM, v2 format).
    pub fn seal(&self, passphrase: &str) -> Result<Vec<u8>> {
        let _ = passphrase;
        todo!("teams crypto")
    }

    pub fn unseal(sealed: &[u8], passphrase: &str) -> Result<Self> {
        let _ = (sealed, passphrase);
        todo!("teams crypto")
    }

    /// Signature over `layout::auth_signing_bytes`.
    pub fn sign_auth_challenge(
        &self,
        challenge_id: Uuid,
        device_id: Uuid,
        nonce: &PublicKey32,
    ) -> Signature64 {
        let _ = (challenge_id, device_id, nonce);
        todo!("teams crypto")
    }
}

/// A team key `TK_v`. Zeroized on drop.
pub struct TeamKey {
    // Implementation-defined fields.
}

impl TeamKey {
    pub fn generate(key_version: u32) -> Self {
        let _ = key_version;
        todo!("teams crypto")
    }

    pub fn key_version(&self) -> u32 {
        todo!("teams crypto")
    }
}

/// Wrap `key` to `recipient`, signed by the wrapping (admin) device.
pub fn wrap_team_key(
    team_id: Uuid,
    key: &TeamKey,
    recipient: &DeviceInfo,
    wrapping_device_id: Uuid,
    wrapper: &DeviceKeys,
) -> Result<WrappedKey> {
    let _ = (team_id, key, recipient, wrapping_device_id, wrapper);
    todo!("teams crypto")
}

/// Verify `wrapped` was signed by `trusted_admin` (a pinned Ed25519 key), then
/// unwrap it with this device's X25519 key.
pub fn unwrap_team_key(
    wrapped: &WrappedKey,
    me: &DeviceKeys,
    trusted_admin: &PublicKey32,
) -> Result<TeamKey> {
    let _ = (wrapped, me, trusted_admin);
    todo!("teams crypto")
}

/// Encrypt and sign a vault payload. `revision` in the result is 0.
pub fn seal_entry(
    team_id: Uuid,
    key: &TeamKey,
    entry_id: Uuid,
    base_revision: u64,
    author_device_id: Uuid,
    author: &DeviceKeys,
    payload: &VaultPayload,
) -> Result<EncryptedEntry> {
    let _ = (
        team_id,
        key,
        entry_id,
        base_revision,
        author_device_id,
        author,
        payload,
    );
    todo!("teams crypto")
}

/// Verify the author's signature, then decrypt. `key` must match
/// `entry.key_version`.
pub fn open_entry(
    team_id: Uuid,
    key: &TeamKey,
    entry: &EncryptedEntry,
    author_ed25519: &PublicKey32,
) -> Result<VaultPayload> {
    let _ = (team_id, key, entry, author_ed25519);
    todo!("teams crypto")
}

/// Fill in `event.signature` over `layout::audit_signing_bytes`.
pub fn sign_audit_event(event: &mut AuditEvent, keys: &DeviceKeys) {
    let _ = (event, keys);
    todo!("teams crypto")
}

pub fn verify_audit_event(event: &AuditEvent, device_ed25519: &PublicKey32) -> Result<()> {
    let _ = (event, device_ed25519);
    todo!("teams crypto")
}

/// OpenSSH-style fingerprint of an Ed25519 key, `SHA256:<base64 no padding>`,
/// shown to admins and pinned by invitees.
pub fn fingerprint(ed25519: &PublicKey32) -> String {
    let _ = ed25519;
    todo!("teams crypto")
}
