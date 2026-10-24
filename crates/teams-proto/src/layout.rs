//! The exact bytes that are authenticated (AEAD associated data), signed
//! (Ed25519) or hashed. Client and server must both build them with these
//! functions; never re-derive a layout by hand.
//!
//! Conventions: every layout starts with a NUL-terminated domain tag, so a
//! signature for one purpose can never verify as another. UUIDs are their 16
//! raw bytes, integers are big-endian, timestamps are Unix milliseconds (i64).

use crate::api::{AuditEvent, EncryptedEntry, RelayMessage, SignedAgentPolicy, WrappedKey};
use crate::wire::{Hash32, Nonce12, PublicKey32};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Domain separation tags.
pub mod domain {
    pub const ENTRY: &[u8] = b"bssh-teams/v1/entry\0";
    pub const KEY_WRAP: &[u8] = b"bssh-teams/v1/key-wrap\0";
    pub const AUDIT: &[u8] = b"bssh-teams/v1/audit\0";
    pub const AUTH: &[u8] = b"bssh-teams/v1/auth\0";
    /// Audit events with an actor or detail, or of an AI kind (tags 5+).
    /// Events of kinds 1 to 4 without either keep the [`AUDIT`] layout.
    pub const AUDIT_V2: &[u8] = b"bssh-teams/v2/audit\0";
    /// Detail of an AI audit event (AEAD associated data).
    pub const AUDIT_DETAIL: &[u8] = b"bssh-teams/v2/audit-detail\0";
    /// Admin-signed agent policy.
    pub const POLICY: &[u8] = b"bssh-teams/v2/agent-policy\0";
    /// Approval relay messages (AEAD associated data and signature).
    pub const RELAY: &[u8] = b"bssh-teams/v2/relay\0";
    /// Content-key wrap inside a relay message (AEAD associated data).
    pub const RELAY_WRAP: &[u8] = b"bssh-teams/v2/relay-wrap\0";
}

/// HKDF-SHA256 `info` strings. See `PROTOCOL.md` for salts and inputs.
pub mod kdf {
    /// Vault key: `HKDF(salt = team_id, ikm = team_key, info = VAULT_KEY ‖ key_version)`.
    pub const VAULT_KEY: &[u8] = b"bssh-teams/v1/vault-key";
    /// Key wrap: `HKDF(salt = ephemeral_pub ‖ recipient_pub, ikm = X25519(eph, recipient), info = KEY_WRAP)`.
    pub const KEY_WRAP: &[u8] = b"bssh-teams/v1/key-wrap";
    /// Audit key: `HKDF(salt = team_id, ikm = team_key, info = AUDIT_KEY ‖ key_version)`.
    pub const AUDIT_KEY: &[u8] = b"bssh-teams/v2/audit-key";
    /// Relay content-key wrap: like [`KEY_WRAP`], with this `info`.
    pub const RELAY_WRAP: &[u8] = b"bssh-teams/v2/relay-wrap";
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

/// Does this event use the original (v1) signing layout? Kinds 1 to 4 with
/// neither actor nor detail do, so every event signed before bssh AI still
/// verifies byte for byte.
pub fn audit_is_v1(event: &AuditEvent) -> bool {
    !event.kind.is_ai() && event.actor.is_none() && event.detail.is_none()
}

fn audit_fixed_fields(b: &mut Vec<u8>, event: &AuditEvent) {
    b.extend_from_slice(event.team_id.as_bytes());
    b.extend_from_slice(event.event_id.as_bytes());
    b.extend_from_slice(event.device_id.as_bytes());
    b.extend_from_slice(&event.seq.to_be_bytes());
    b.push(event.kind.tag());
    b.extend_from_slice(event.entry_id.unwrap_or(Uuid::nil()).as_bytes());
    b.extend_from_slice(&event.occurred_at.timestamp_millis().to_be_bytes());
    b.extend_from_slice(event.prev_hash.as_bytes());
}

fn push_str(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&(s.len() as u32).to_be_bytes());
    b.extend_from_slice(s.as_bytes());
}

/// Bytes a device signs for an audit event.
///
/// v1 (kinds 1 to 4, no actor, no detail):
/// `AUDIT ‖ team_id ‖ event_id ‖ device_id ‖ seq ‖ kind ‖ entry_id|zeros ‖ occurred_at_ms ‖ prev_hash`.
///
/// v2 (everything else): `AUDIT_V2 ‖` the same fixed fields `‖ actor ‖ detail`, where
/// `actor` is `0x00`, or `0x01 ‖ kind (1 human, 2 agent) ‖ len32 ‖ name ‖ flag ‖ [len32 ‖ on_behalf_of]`,
/// and `detail` is `0x00`, or `0x01 ‖ key_version ‖ nonce ‖ SHA-256(ciphertext)`.
pub fn audit_signing_bytes(event: &AuditEvent) -> Vec<u8> {
    if audit_is_v1(event) {
        let mut b = Vec::with_capacity(domain::AUDIT.len() + 16 * 4 + 8 + 1 + 8 + 32);
        b.extend_from_slice(domain::AUDIT);
        audit_fixed_fields(&mut b, event);
        return b;
    }
    let mut b = Vec::with_capacity(domain::AUDIT_V2.len() + 16 * 4 + 8 + 1 + 8 + 32 + 64);
    b.extend_from_slice(domain::AUDIT_V2);
    audit_fixed_fields(&mut b, event);
    match &event.actor {
        None => b.push(0),
        Some(a) => {
            b.push(1);
            b.push(match a.kind {
                crate::api::MemberKind::Human => 1,
                crate::api::MemberKind::Agent => 2,
            });
            push_str(&mut b, &a.name);
            match &a.on_behalf_of {
                None => b.push(0),
                Some(o) => {
                    b.push(1);
                    push_str(&mut b, o);
                }
            }
        }
    }
    match &event.detail {
        None => b.push(0),
        Some(d) => {
            b.push(1);
            b.extend_from_slice(&d.key_version.to_be_bytes());
            b.extend_from_slice(d.nonce.as_bytes());
            b.extend_from_slice(&sha256(&d.ciphertext.0));
        }
    }
    b
}

/// AEAD associated data of an audit event's encrypted detail:
/// `AUDIT_DETAIL ‖ team_id ‖ event_id ‖ device_id ‖ kind ‖ key_version`.
pub fn audit_detail_aad(event: &AuditEvent, key_version: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::AUDIT_DETAIL.len() + 16 * 3 + 1 + 4);
    b.extend_from_slice(domain::AUDIT_DETAIL);
    b.extend_from_slice(event.team_id.as_bytes());
    b.extend_from_slice(event.event_id.as_bytes());
    b.extend_from_slice(event.device_id.as_bytes());
    b.push(event.kind.tag());
    b.extend_from_slice(&key_version.to_be_bytes());
    b
}

/// Bytes an admin device signs for an agent policy:
/// `POLICY ‖ team_id ‖ agent_device_id ‖ signer_device_id ‖ version ‖ signed_at_ms ‖ len32 ‖ name ‖ SHA-256(policy_json)`.
pub fn policy_signing_bytes(team_id: Uuid, policy: &SignedAgentPolicy) -> Vec<u8> {
    let mut b =
        Vec::with_capacity(domain::POLICY.len() + 16 * 3 + 8 + 8 + 4 + policy.name.len() + 32);
    b.extend_from_slice(domain::POLICY);
    b.extend_from_slice(team_id.as_bytes());
    b.extend_from_slice(policy.agent_device_id.as_bytes());
    b.extend_from_slice(policy.signer_device_id.as_bytes());
    b.extend_from_slice(&policy.version.to_be_bytes());
    b.extend_from_slice(&policy.signed_at.timestamp_millis().to_be_bytes());
    push_str(&mut b, &policy.name);
    b.extend_from_slice(&sha256(policy.policy_json.as_bytes()));
    b
}

/// AEAD associated data of a relay message's payload:
/// `RELAY ‖ team_id ‖ message_id ‖ request_id ‖ kind ‖ sender_device_id ‖ created_at_ms ‖ expires_at_ms`.
pub fn relay_aad(m: &RelayMessage) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::RELAY.len() + 16 * 4 + 1 + 16);
    b.extend_from_slice(domain::RELAY);
    b.extend_from_slice(m.team_id.as_bytes());
    b.extend_from_slice(m.message_id.as_bytes());
    b.extend_from_slice(m.request_id.as_bytes());
    b.push(m.kind.tag());
    b.extend_from_slice(m.sender_device_id.as_bytes());
    b.extend_from_slice(&m.created_at.timestamp_millis().to_be_bytes());
    b.extend_from_slice(&m.expires_at.timestamp_millis().to_be_bytes());
    b
}

/// AEAD associated data of one wrapped content key: `RELAY_WRAP ‖ message_id ‖ recipient_device_id`.
pub fn relay_wrap_aad(message_id: Uuid, recipient_device_id: Uuid) -> Vec<u8> {
    let mut b = Vec::with_capacity(domain::RELAY_WRAP.len() + 32);
    b.extend_from_slice(domain::RELAY_WRAP);
    b.extend_from_slice(message_id.as_bytes());
    b.extend_from_slice(recipient_device_id.as_bytes());
    b
}

/// Bytes the sender device signs for a relay message:
/// `relay_aad ‖ nonce ‖ SHA-256(ciphertext) ‖ SHA-256(recipients)`, where `recipients` is, per
/// recipient in order, `device_id ‖ ephemeral_public ‖ nonce ‖ SHA-256(wrapped_key)`.
/// The `RELAY` domain makes it unusable as any other signature.
pub fn relay_signing_bytes(m: &RelayMessage) -> Vec<u8> {
    let mut b = relay_aad(m);
    b.extend_from_slice(m.nonce.as_bytes());
    b.extend_from_slice(&sha256(&m.ciphertext.0));
    let mut r = Vec::with_capacity(m.recipients.len() * 92);
    for rec in &m.recipients {
        r.extend_from_slice(rec.device_id.as_bytes());
        r.extend_from_slice(rec.ephemeral_public.as_bytes());
        r.extend_from_slice(rec.nonce.as_bytes());
        r.extend_from_slice(&sha256(&rec.wrapped_key.0));
    }
    b.extend_from_slice(&sha256(&r));
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
            actor: None,
            detail: None,
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
        let all = [
            domain::ENTRY,
            domain::KEY_WRAP,
            domain::AUDIT,
            domain::AUTH,
            domain::AUDIT_V2,
            domain::AUDIT_DETAIL,
            domain::POLICY,
            domain::RELAY,
            domain::RELAY_WRAP,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(!a.starts_with(b) && !b.starts_with(a));
            }
        }
    }

    #[test]
    fn v1_audit_layout_is_frozen_for_old_events() {
        // Hand-built per the PROTOCOL.md v1 layout: an event signed before
        // bssh AI must produce exactly these bytes forever.
        let ev = sample_event();
        let mut want = Vec::new();
        want.extend_from_slice(b"bssh-teams/v1/audit\0");
        want.extend_from_slice(&[1; 16]);
        want.extend_from_slice(&[6; 16]);
        want.extend_from_slice(&[4; 16]);
        want.extend_from_slice(&7u64.to_be_bytes());
        want.push(1);
        want.extend_from_slice(&[2; 16]);
        want.extend_from_slice(&1_700_000_000_123i64.to_be_bytes());
        want.extend_from_slice(&[8; 32]);
        assert_eq!(audit_signing_bytes(&ev), want);
        assert!(audit_is_v1(&ev));
    }

    #[test]
    fn ai_events_use_v2_and_bind_actor_and_detail() {
        use crate::api::{AuditActor, AuditDetail, MemberKind};
        let mut ev = sample_event();
        ev.kind = AuditKind::AiCall;
        let bare = audit_signing_bytes(&ev);
        assert!(bare.starts_with(domain::AUDIT_V2));
        assert!(!audit_is_v1(&ev));

        let mut with_actor = ev.clone();
        with_actor.actor = Some(AuditActor {
            kind: MemberKind::Agent,
            name: "deployer".into(),
            on_behalf_of: None,
        });
        assert_ne!(bare, audit_signing_bytes(&with_actor));
        let mut other = with_actor.clone();
        other.actor.as_mut().unwrap().on_behalf_of = Some("alice".into());
        assert_ne!(
            audit_signing_bytes(&with_actor),
            audit_signing_bytes(&other)
        );

        let mut with_detail = with_actor.clone();
        with_detail.detail = Some(AuditDetail {
            key_version: 1,
            nonce: Fixed([3; 12]),
            ciphertext: Bytes(b"x".to_vec()),
        });
        assert_ne!(
            audit_signing_bytes(&with_actor),
            audit_signing_bytes(&with_detail)
        );
        let mut tampered = with_detail.clone();
        tampered.detail.as_mut().unwrap().ciphertext = Bytes(b"y".to_vec());
        assert_ne!(
            audit_signing_bytes(&with_detail),
            audit_signing_bytes(&tampered)
        );

        // An old kind that gains an actor moves to v2.
        let mut old_kind = sample_event();
        old_kind.actor = with_actor.actor.clone();
        assert!(audit_signing_bytes(&old_kind).starts_with(domain::AUDIT_V2));
    }

    #[test]
    fn relay_signature_binds_every_field() {
        use crate::api::{RelayKind, RelayRecipient};
        let m = RelayMessage {
            message_id: id(1),
            team_id: id(2),
            request_id: id(3),
            kind: RelayKind::Request,
            sender_device_id: id(4),
            created_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            expires_at: Utc.timestamp_millis_opt(1_700_000_600_000).unwrap(),
            nonce: Fixed([1; 12]),
            ciphertext: Bytes(b"ct".to_vec()),
            recipients: vec![RelayRecipient {
                device_id: id(5),
                ephemeral_public: Fixed([2; 32]),
                nonce: Fixed([3; 12]),
                wrapped_key: Bytes(b"wk".to_vec()),
            }],
            signature: Fixed::ZERO,
        };
        let base = relay_signing_bytes(&m);
        let mut v = m.clone();
        v.kind = RelayKind::Decision;
        assert_ne!(base, relay_signing_bytes(&v));
        let mut v = m.clone();
        v.expires_at += chrono::Duration::seconds(1);
        assert_ne!(base, relay_signing_bytes(&v));
        let mut v = m.clone();
        v.recipients[0].wrapped_key = Bytes(b"zz".to_vec());
        assert_ne!(base, relay_signing_bytes(&v));
        let mut v = m.clone();
        v.recipients.push(v.recipients[0].clone());
        assert_ne!(base, relay_signing_bytes(&v));
        let mut v = m.clone();
        v.ciphertext = Bytes(b"cx".to_vec());
        assert_ne!(base, relay_signing_bytes(&v));
        assert_ne!(relay_aad(&m), relay_wrap_aad(id(1), id(5)));
    }
}
