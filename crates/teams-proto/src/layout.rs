//! The exact bytes that are authenticated (AEAD associated data), signed
//! (Ed25519) or hashed. Client and server must both build them with these
//! functions; never re-derive a layout by hand.
//!
//! Conventions: every layout starts with a NUL-terminated domain tag, so a
//! signature for one purpose can never verify as another. UUIDs are their 16
//! raw bytes, integers are big-endian, timestamps are Unix milliseconds (i64).

use crate::api::{AuditEvent, EncryptedEntry, WrappedKey};
use crate::wire::{Hash32, Nonce12, PublicKey32};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Domain separation tags.
pub mod domain {
    pub const ENTRY: &[u8] = b"bssh-teams/v1/entry\0";
    pub const KEY_WRAP: &[u8] = b"bssh-teams/v1/key-wrap\0";
    pub const AUDIT: &[u8] = b"bssh-teams/v1/audit\0";
    pub const AUTH: &[u8] = b"bssh-teams/v1/auth\0";
}

/// HKDF-SHA256 `info` strings. See `PROTOCOL.md` for salts and inputs.
pub mod kdf {
    /// Vault key: `HKDF(salt = team_id, ikm = team_key, info = VAULT_KEY ‖ key_version)`.
    pub const VAULT_KEY: &[u8] = b"bssh-teams/v1/vault-key";
    /// Key wrap: `HKDF(salt = ephemeral_pub ‖ recipient_pub, ikm = X25519(eph, recipient), info = KEY_WRAP)`.
    pub const KEY_WRAP: &[u8] = b"bssh-teams/v1/key-wrap";
}

fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// AEAD associated data for a vault entry.
///
/// `ENTRY ‖ team_id ‖ entry_id ‖ base_revision ‖ key_version ‖ author_device_id`
pub fn entry_aad(
    team_id: Uuid,
    entry_id: Uuid,
    base_revision: u64,
    key_version: u32,
    author_device_id: Uuid,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::ENTRY.len() + 16 * 3 + 8 + 4);
    b.extend_from_slice(domain::ENTRY);
    b.extend_from_slice(team_id.as_bytes());
    b.extend_from_slice(entry_id.as_bytes());
    b.extend_from_slice(&base_revision.to_be_bytes());
    b.extend_from_slice(&key_version.to_be_bytes());
    b.extend_from_slice(author_device_id.as_bytes());
    b
}

/// Bytes the author device signs for a vault entry:
/// `entry_aad ‖ nonce ‖ SHA-256(ciphertext)`.
///
/// The server verifies this signature before accepting a push, which stops a
/// removed member's device from writing even though it can't read new entries.
pub fn entry_signing_bytes(team_id: Uuid, entry: &EncryptedEntry) -> Vec<u8> {
    let mut b = entry_aad(
        team_id,
        entry.entry_id,
        entry.base_revision,
        entry.key_version,
        entry.author_device_id,
    );
    b.extend_from_slice(entry.nonce.as_bytes());
    b.extend_from_slice(&sha256(&entry.ciphertext.0));
    b
}

/// AEAD associated data for a wrapped team key.
///
/// `KEY_WRAP ‖ team_id ‖ key_version ‖ recipient_device_id ‖ wrapping_device_id`
pub fn key_wrap_aad(
    team_id: Uuid,
    key_version: u32,
    recipient_device_id: Uuid,
    wrapping_device_id: Uuid,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::KEY_WRAP.len() + 16 * 3 + 4);
    b.extend_from_slice(domain::KEY_WRAP);
    b.extend_from_slice(team_id.as_bytes());
    b.extend_from_slice(&key_version.to_be_bytes());
    b.extend_from_slice(recipient_device_id.as_bytes());
    b.extend_from_slice(wrapping_device_id.as_bytes());
    b
}

/// Bytes the wrapping (admin) device signs:
/// `key_wrap_aad ‖ ephemeral_public ‖ nonce ‖ SHA-256(ciphertext)`.
///
/// Recipients only accept a wrapped key whose signer is a pinned admin device,
/// so the server can't hand them a team key of its own choosing.
pub fn key_wrap_signing_bytes(key: &WrappedKey) -> Vec<u8> {
    let mut b = key_wrap_aad(
        key.team_id,
        key.key_version,
        key.recipient_device_id,
        key.wrapping_device_id,
    );
    b.extend_from_slice(key.ephemeral_public.as_bytes());
    b.extend_from_slice(key.nonce.as_bytes());
    b.extend_from_slice(&sha256(&key.ciphertext.0));
    b
}

/// Bytes a device signs to answer an auth challenge:
/// `AUTH ‖ challenge_id ‖ device_id ‖ challenge_nonce`.
pub fn auth_signing_bytes(challenge_id: Uuid, device_id: Uuid, nonce: &PublicKey32) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::AUTH.len() + 16 * 2 + 32);
    b.extend_from_slice(domain::AUTH);
    b.extend_from_slice(challenge_id.as_bytes());
    b.extend_from_slice(device_id.as_bytes());
    b.extend_from_slice(nonce.as_bytes());
    b
}

/// Bytes a device signs for an audit event:
/// `AUDIT ‖ team_id ‖ event_id ‖ device_id ‖ seq ‖ kind ‖ entry_id|zeros ‖ occurred_at_ms ‖ prev_hash`.
pub fn audit_signing_bytes(event: &AuditEvent) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::AUDIT.len() + 16 * 4 + 8 + 1 + 8 + 32);
    b.extend_from_slice(domain::AUDIT);
    b.extend_from_slice(event.team_id.as_bytes());
    b.extend_from_slice(event.event_id.as_bytes());
    b.extend_from_slice(event.device_id.as_bytes());
    b.extend_from_slice(&event.seq.to_be_bytes());
    b.push(event.kind.tag());
    b.extend_from_slice(event.entry_id.unwrap_or(Uuid::nil()).as_bytes());
    b.extend_from_slice(&event.occurred_at.timestamp_millis().to_be_bytes());
    b.extend_from_slice(event.prev_hash.as_bytes());
    b
}

/// Chain hash of an audit event: `SHA-256(audit_signing_bytes ‖ signature)`.
///
/// Each device keeps its own chain: event `seq = n` carries
/// `prev_hash = audit_event_hash(event n-1)`, and `seq = 0` carries
/// [`AUDIT_GENESIS`]. A gap, reorder or edit breaks the chain.
pub fn audit_event_hash(event: &AuditEvent) -> Hash32 {
    let mut h = Sha256::new();
    h.update(audit_signing_bytes(event));
    h.update(event.signature.as_bytes());
    crate::wire::Fixed(h.finalize().into())
}

/// `prev_hash` of the first event (`seq = 0`) in a device's chain.
pub const AUDIT_GENESIS: Hash32 = crate::wire::Fixed([0u8; 32]);

/// Length of an AES-256-GCM nonce, for implementations sizing buffers.
pub const NONCE_LEN: usize = std::mem::size_of::<Nonce12>();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AuditKind;
    use crate::wire::{Bytes, Fixed};
    use chrono::{TimeZone, Utc};

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn sample_entry() -> EncryptedEntry {
        EncryptedEntry {
            entry_id: id(2),
            revision: 0,
            base_revision: 5,
            key_version: 3,
            author_device_id: id(4),
            nonce: Fixed([9; 12]),
            ciphertext: Bytes(b"ciphertext".to_vec()),
            signature: Fixed::ZERO,
        }
    }

    fn sample_event() -> AuditEvent {
        AuditEvent {
            event_id: id(6),
            team_id: id(1),
            device_id: id(4),
            seq: 7,
            kind: AuditKind::SessionStart,
            entry_id: Some(id(2)),
            occurred_at: Utc.timestamp_millis_opt(1_700_000_000_123).unwrap(),
            prev_hash: Fixed([8; 32]),
            signature: Fixed([5; 64]),
        }
    }

    // Golden vectors: if one of these changes, the wire format changed and
    // PROTOCOL_VERSION must be bumped.
    #[test]
    fn entry_aad_layout_is_stable() {
        let aad = entry_aad(id(1), id(2), 5, 3, id(4));
        assert_eq!(aad.len(), domain::ENTRY.len() + 60);
        assert_eq!(
            hex(&aad),
            concat!(
                "627373682d7465616d732f76312f656e74727900",
                "01010101010101010101010101010101",
                "02020202020202020202020202020202",
                "0000000000000005",
                "00000003",
                "04040404040404040404040404040404"
            )
        );
    }

    #[test]
    fn entry_signing_bytes_bind_nonce_and_ciphertext() {
        let e = sample_entry();
        let base = entry_signing_bytes(id(1), &e);
        assert_eq!(base.len(), domain::ENTRY.len() + 60 + 12 + 32);

        let mut other_ct = e.clone();
        other_ct.ciphertext = Bytes(b"tampered!!".to_vec());
        assert_ne!(base, entry_signing_bytes(id(1), &other_ct));

        // The server-assigned revision is not signed: it's set after signing.
        let mut assigned = e.clone();
        assigned.revision = 42;
        assert_eq!(base, entry_signing_bytes(id(1), &assigned));

        assert_ne!(base, entry_signing_bytes(id(9), &e), "team id is bound");
    }

    #[test]
    fn audit_layout_is_stable_and_chains() {
        let ev = sample_event();
        let bytes = audit_signing_bytes(&ev);
        assert_eq!(bytes.len(), domain::AUDIT.len() + 16 * 4 + 8 + 1 + 8 + 32);
        assert_eq!(
            bytes[domain::AUDIT.len() + 48 + 8],
            AuditKind::SessionStart.tag()
        );

        let h = audit_event_hash(&ev);
        let mut resigned = ev.clone();
        resigned.signature = Fixed([6; 64]);
        assert_ne!(
            h,
            audit_event_hash(&resigned),
            "signature is part of the chain"
        );
    }

    #[test]
    fn domains_are_distinct() {
        let all = [domain::ENTRY, domain::KEY_WRAP, domain::AUDIT, domain::AUTH];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(!a.starts_with(b) && !b.starts_with(a));
            }
        }
    }
}
