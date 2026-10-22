//! Seal and open vault entries behind a small trait, so the sync engine can be
//! tested without real cryptography.

use super::crypto::{self, DeviceKeys, TeamKey};
use anyhow::{anyhow, Result};
use bssh_teams_proto::api::{AuditEvent, EncryptedEntry, VaultPayload, WrappedKey};
use bssh_teams_proto::wire::PublicKey32;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use uuid::Uuid;

/// What the sync engine and local reads need from the crypto layer.
pub trait VaultCodec {
    /// Encrypt and sign `payload` as an edit based on `base_revision`, under
    /// the newest key. The result has `revision` 0.
    fn seal(
        &self,
        entry_id: Uuid,
        base_revision: u64,
        payload: &VaultPayload,
    ) -> Result<EncryptedEntry>;

    /// Verify the author's signature, then decrypt.
    fn open(&self, entry: &EncryptedEntry) -> Result<VaultPayload>;

    /// The newest team-key version this device holds (the one `seal` uses).
    fn key_version(&self) -> u32;

    /// This device's id (author of sealed entries and audit events).
    fn device_id(&self) -> Uuid;

    /// Fill in `event.signature`.
    fn sign_audit(&self, event: &mut AuditEvent);

    /// Whether `device_id` is a known device of this team (current or former
    /// member). Entries from unknown authors are rejected on pull.
    fn knows_author(&self, device_id: Uuid) -> bool;
}

/// Production codec: delegates to [`crypto`].
pub struct CryptoCodec {
    team_id: Uuid,
    device_id: Uuid,
    /// Shared with the HTTP client, which signs auth challenges with it.
    device_keys: Arc<DeviceKeys>,
    team_keys: BTreeMap<u32, TeamKey>,
    /// Author device id -> Ed25519 public key. Includes this device and former
    /// members' devices, so their old entries stay verifiable.
    author_keys: HashMap<Uuid, PublicKey32>,
}

impl CryptoCodec {
    pub fn new(
        team_id: Uuid,
        device_id: Uuid,
        device_keys: impl Into<Arc<DeviceKeys>>,
        team_keys: Vec<TeamKey>,
        author_keys: HashMap<Uuid, PublicKey32>,
    ) -> Result<Self> {
        let device_keys = device_keys.into();
        if team_keys.is_empty() {
            return Err(anyhow!("no team key available for this device"));
        }
        let mut author_keys = author_keys;
        author_keys.insert(device_id, device_keys.ed25519_public());
        Ok(Self {
            team_id,
            device_id,
            team_keys: team_keys
                .into_iter()
                .map(|k| (k.key_version(), k))
                .collect(),
            device_keys,
            author_keys,
        })
    }

    /// Register another device's signing key (e.g. after reading the member list).
    pub fn add_author(&mut self, device_id: Uuid, ed25519: PublicKey32) {
        self.author_keys.insert(device_id, ed25519);
    }

    fn newest(&self) -> Result<&TeamKey> {
        self.team_keys
            .values()
            .next_back()
            .ok_or_else(|| anyhow!("no team key"))
    }
}

/// Why a wrapped key from the server was refused. A malicious server could
/// replay an old but validly signed wrap, so these are hard errors: callers
/// must never fall back to using the key anyway.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyCheckError {
    #[error("wrapped key (version {0}) belongs to a different team")]
    WrongTeam(u32),
    #[error("wrapped key (version {0}) is addressed to a different device")]
    WrongRecipient(u32),
    #[error("wrapped key has invalid key version 0")]
    VersionZero,
    #[error("server sent two different wrapped keys for version {0}")]
    DuplicateVersion(u32),
    #[error("server sent a different wrapped key for already stored version {0}")]
    ConflictsWithStored(u32),
    #[error("wrapped key version {version} is older than stored version {newest_stored}")]
    StaleVersion { version: u32, newest_stored: u32 },
    #[error("wrapped key version {version} is newer than the team's current version {current}")]
    AheadOfServer { version: u32, current: u32 },
}

/// Validate wrapped keys before unwrapping or storing them. Returns the keys
/// not already stored (identical re-deliveries are accepted and skipped).
///
/// - team id and recipient device must match;
/// - a stored version is never replaced by a different wrap;
/// - a version older than the newest stored one can't be added later;
/// - with `current_version` (from `TeamInfo`), no version beyond it.
pub fn check_incoming_keys(
    team_id: Uuid,
    my_device_id: Uuid,
    stored: &[WrappedKey],
    incoming: &[WrappedKey],
    current_version: Option<u32>,
) -> std::result::Result<Vec<WrappedKey>, KeyCheckError> {
    let newest_stored = stored.iter().map(|k| k.key_version).max();
    let mut fresh: Vec<WrappedKey> = Vec::new();
    for k in incoming {
        if k.team_id != team_id {
            return Err(KeyCheckError::WrongTeam(k.key_version));
        }
        if k.recipient_device_id != my_device_id {
            return Err(KeyCheckError::WrongRecipient(k.key_version));
        }
        if k.key_version == 0 {
            return Err(KeyCheckError::VersionZero);
        }
        if let Some(prev) = fresh.iter().find(|f| f.key_version == k.key_version) {
            if prev != k {
                return Err(KeyCheckError::DuplicateVersion(k.key_version));
            }
            continue;
        }
        if let Some(have) = stored.iter().find(|s| s.key_version == k.key_version) {
            if have != k {
                return Err(KeyCheckError::ConflictsWithStored(k.key_version));
            }
            continue;
        }
        if let Some(newest) = newest_stored {
            if k.key_version < newest {
                return Err(KeyCheckError::StaleVersion {
                    version: k.key_version,
                    newest_stored: newest,
                });
            }
        }
        if let Some(current) = current_version {
            if k.key_version > current {
                return Err(KeyCheckError::AheadOfServer {
                    version: k.key_version,
                    current,
                });
            }
        }
        fresh.push(k.clone());
    }
    Ok(fresh)
}

/// Unwrap every key with this device's keys, accepting only wraps signed by
/// the pinned trust root and passing [`check_incoming_keys`] first. Unwrapped
/// keys exist only in memory.
pub fn unwrap_all(
    team_id: Uuid,
    my_device_id: Uuid,
    wrapped: &[WrappedKey],
    me: &DeviceKeys,
    trust_root: &PublicKey32,
) -> Result<Vec<TeamKey>> {
    check_incoming_keys(team_id, my_device_id, &[], wrapped, None)?;
    wrapped
        .iter()
        .map(|w| crypto::unwrap_team_key(w, me, trust_root).map_err(Into::into))
        .collect()
}

impl VaultCodec for CryptoCodec {
    fn seal(
        &self,
        entry_id: Uuid,
        base_revision: u64,
        payload: &VaultPayload,
    ) -> Result<EncryptedEntry> {
        Ok(crypto::seal_entry(
            self.team_id,
            self.newest()?,
            entry_id,
            base_revision,
            self.device_id,
            &self.device_keys,
            payload,
        )?)
    }

    fn open(&self, entry: &EncryptedEntry) -> Result<VaultPayload> {
        let key = self
            .team_keys
            .get(&entry.key_version)
            .ok_or_else(|| anyhow!("no team key for version {}", entry.key_version))?;
        let author = self
            .author_keys
            .get(&entry.author_device_id)
            .ok_or_else(|| anyhow!("unknown author device {}", entry.author_device_id))?;
        Ok(crypto::open_entry(self.team_id, key, entry, author)?)
    }

    fn key_version(&self) -> u32 {
        self.team_keys.keys().next_back().copied().unwrap_or(0)
    }

    fn device_id(&self) -> Uuid {
        self.device_id
    }

    fn sign_audit(&self, event: &mut AuditEvent) {
        crypto::sign_audit_event(event, &self.device_keys);
    }

    fn knows_author(&self, device_id: Uuid) -> bool {
        self.author_keys.contains_key(&device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::wire::{Bytes, Fixed};

    fn wk(team: Uuid, dev: Uuid, v: u32, tag: u8) -> WrappedKey {
        WrappedKey {
            team_id: team,
            key_version: v,
            recipient_device_id: dev,
            wrapping_device_id: Uuid::nil(),
            ephemeral_public: Fixed([tag; 32]),
            nonce: Fixed([tag; 12]),
            ciphertext: Bytes(vec![tag; 48]),
            signature: Fixed([tag; 64]),
        }
    }

    #[test]
    fn rejects_wrong_team_and_wrong_recipient() {
        let (t, d) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(
            check_incoming_keys(t, d, &[], &[wk(Uuid::new_v4(), d, 1, 1)], None),
            Err(KeyCheckError::WrongTeam(1))
        );
        assert_eq!(
            check_incoming_keys(t, d, &[], &[wk(t, Uuid::new_v4(), 1, 1)], None),
            Err(KeyCheckError::WrongRecipient(1))
        );
        assert_eq!(
            check_incoming_keys(t, d, &[], &[wk(t, d, 0, 1)], None),
            Err(KeyCheckError::VersionZero)
        );
    }

    #[test]
    fn stored_version_is_never_replaced_but_redelivery_is_fine() {
        let (t, d) = (Uuid::new_v4(), Uuid::new_v4());
        let stored = vec![wk(t, d, 1, 1), wk(t, d, 2, 2)];
        assert_eq!(
            check_incoming_keys(t, d, &stored, &[wk(t, d, 2, 9)], None),
            Err(KeyCheckError::ConflictsWithStored(2))
        );
        assert_eq!(
            check_incoming_keys(t, d, &stored, &stored, None),
            Ok(vec![])
        );
    }

    #[test]
    fn older_version_cannot_arrive_after_a_newer_one() {
        let (t, d) = (Uuid::new_v4(), Uuid::new_v4());
        let stored = vec![wk(t, d, 3, 3)];
        assert_eq!(
            check_incoming_keys(t, d, &stored, &[wk(t, d, 2, 2)], None),
            Err(KeyCheckError::StaleVersion {
                version: 2,
                newest_stored: 3
            })
        );
        // A fresh device may legitimately receive several versions at once.
        let all = [wk(t, d, 1, 1), wk(t, d, 2, 2)];
        assert_eq!(
            check_incoming_keys(t, d, &[], &all, Some(2)).unwrap().len(),
            2
        );
        // Newer than the team's current version is refused.
        assert_eq!(
            check_incoming_keys(t, d, &[], &[wk(t, d, 5, 5)], Some(2)),
            Err(KeyCheckError::AheadOfServer {
                version: 5,
                current: 2
            })
        );
        assert_eq!(
            check_incoming_keys(t, d, &[], &[wk(t, d, 1, 1), wk(t, d, 1, 2)], None),
            Err(KeyCheckError::DuplicateVersion(1))
        );
    }

    /// The whole client-side path with real crypto, no fakes: an admin wraps
    /// the team key to a member's device, the member unwraps it against the
    /// pinned admin key, seals an entry, and the admin opens it.
    #[test]
    fn real_crypto_round_trip_between_two_devices() {
        use bssh_teams_proto::api::{ConnectionRecord, DeviceInfo};

        let team = Uuid::new_v4();
        let (admin_id, member_id) = (Uuid::new_v4(), Uuid::new_v4());
        let admin = DeviceKeys::generate();
        let member = DeviceKeys::generate();
        let member_info = DeviceInfo {
            device_id: member_id,
            account_id: Uuid::new_v4(),
            name: "laptop".into(),
            x25519_public: member.x25519_public(),
            ed25519_public: member.ed25519_public(),
            created_at: chrono::Utc::now(),
            revoked_at: None,
        };

        let tk = TeamKey::generate(1);
        let wrapped = crypto::wrap_team_key(team, &tk, &member_info, admin_id, &admin).unwrap();
        let trust_root = admin.ed25519_public();
        let member_keys = unwrap_all(
            team,
            member_id,
            std::slice::from_ref(&wrapped),
            &member,
            &trust_root,
        )
        .expect("member unwraps a key signed by the pinned admin");

        // A wrap replayed for another team or device is refused before unwrapping.
        assert!(unwrap_all(
            Uuid::new_v4(),
            member_id,
            std::slice::from_ref(&wrapped),
            &member,
            &trust_root
        )
        .is_err());
        assert!(unwrap_all(team, Uuid::new_v4(), &[wrapped], &member, &trust_root).is_err());

        let member_codec = CryptoCodec::new(
            team,
            member_id,
            member,
            member_keys,
            HashMap::from([(admin_id, trust_root)]),
        )
        .unwrap();
        let record = ConnectionRecord {
            name: "db-prod".into(),
            host: "10.0.0.5".into(),
            user: "ops".into(),
            port: 22,
            bastion: None,
            bastion_user: None,
            use_kerberos: false,
            tags: vec!["prod".into()],
            notes: None,
        };
        let entry = member_codec
            .seal(Uuid::new_v4(), 0, &VaultPayload::Connection(record.clone()))
            .unwrap();
        assert!(!String::from_utf8_lossy(&entry.ciphertext.0).contains("10.0.0.5"));

        let member_pub = member_codec.device_keys.ed25519_public();
        let admin_codec = CryptoCodec::new(
            team,
            admin_id,
            admin,
            vec![tk],
            HashMap::from([(member_id, member_pub)]),
        )
        .unwrap();
        assert_eq!(
            admin_codec.open(&entry).unwrap(),
            VaultPayload::Connection(record)
        );
    }
}
