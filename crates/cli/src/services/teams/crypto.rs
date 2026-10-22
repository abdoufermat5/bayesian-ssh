//! Client-side cryptography for bssh Teams.
//!
//! Implements the recipes in `crates/teams-proto/PROTOCOL.md` exactly. Every
//! authenticated or signed byte string comes from `bssh_teams_proto::layout`;
//! never build one by hand here.

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use bssh_teams_proto::api::{
    AuditEvent, DeviceInfo, EncryptedEntry, NewDevice, VaultPayload, WrappedKey,
};
use bssh_teams_proto::layout;
use bssh_teams_proto::wire::{Bytes, Fixed, PublicKey32, Signature64};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey as XPublic, StaticSecret};
use zeroize::Zeroizing;

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

/// Version byte of the serialized (pre-encryption) device secrets.
const DEVICE_SECRETS_V1: u8 = 1;

fn random_bytes<const N: usize>() -> Zeroizing<[u8; N]> {
    let mut b = Zeroizing::new([0u8; N]);
    OsRng.fill_bytes(&mut b[..]);
    b
}

fn random_nonce() -> [u8; 12] {
    let mut n = [0u8; 12];
    OsRng.fill_bytes(&mut n);
    n
}

fn aes_encrypt(key: &[u8; 32], nonce: &[u8; 12], msg: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
        .encrypt(Nonce::from_slice(nonce), Payload { msg, aad })
        .map_err(|_| TeamsCryptoError::Malformed("plaintext (encryption failed)"))
}

fn aes_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ct: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| TeamsCryptoError::Decrypt)
}

fn verify(pk: &PublicKey32, msg: &[u8], sig: &Signature64) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pk.as_bytes()) else {
        return false;
    };
    let sig = ed25519_dalek::Signature::from_bytes(sig.as_bytes());
    vk.verify_strict(msg, &sig).is_ok()
}

/// A device's two private keys (X25519 for receiving team keys, Ed25519 for
/// signing). Zeroized on drop; never serialized except via [`DeviceKeys::seal`].
/// Deliberately not `Clone` and its `Debug` is redacted.
pub struct DeviceKeys {
    x25519: StaticSecret,
    ed25519: SigningKey,
}

impl std::fmt::Debug for DeviceKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceKeys(<redacted>)")
    }
}

impl DeviceKeys {
    /// Fresh keys from the OS RNG.
    pub fn generate() -> Self {
        let x = random_bytes::<32>();
        let e = random_bytes::<32>();
        Self::from_secrets(&x, &e)
    }

    fn from_secrets(x25519: &[u8; 32], ed25519_seed: &[u8; 32]) -> Self {
        Self {
            x25519: StaticSecret::from(*x25519),
            ed25519: SigningKey::from_bytes(ed25519_seed),
        }
    }

    /// Public half, as registered with the server.
    pub fn to_new_device(&self, name: &str) -> NewDevice {
        NewDevice {
            name: name.to_string(),
            x25519_public: self.x25519_public(),
            ed25519_public: self.ed25519_public(),
        }
    }

    pub fn ed25519_public(&self) -> PublicKey32 {
        Fixed(self.ed25519.verifying_key().to_bytes())
    }

    pub fn x25519_public(&self) -> PublicKey32 {
        Fixed(XPublic::from(&self.x25519).to_bytes())
    }

    /// Encrypt both private keys with `passphrase` using `services::crypto`
    /// (Argon2id + AES-256-GCM, v2 format). Plaintext layout:
    /// `0x01 ‖ x25519_secret (32) ‖ ed25519_seed (32)`.
    pub fn seal(&self, passphrase: &str) -> Result<Vec<u8>> {
        let mut plain = Zeroizing::new(Vec::with_capacity(65));
        plain.push(DEVICE_SECRETS_V1);
        plain.extend_from_slice(&self.x25519.to_bytes());
        plain.extend_from_slice(&self.ed25519.to_bytes());
        crate::services::crypto::encrypt_data(&plain, passphrase)
            .map_err(|e| TeamsCryptoError::Sealed(e.to_string()))
    }

    pub fn unseal(sealed: &[u8], passphrase: &str) -> Result<Self> {
        let plain = Zeroizing::new(
            crate::services::crypto::decrypt_data(sealed, passphrase)
                .map_err(|e| TeamsCryptoError::Sealed(e.to_string()))?,
        );
        if plain.len() != 65 || plain[0] != DEVICE_SECRETS_V1 {
            return Err(TeamsCryptoError::Sealed(
                "unrecognised device key format".into(),
            ));
        }
        let mut x = Zeroizing::new([0u8; 32]);
        let mut e = Zeroizing::new([0u8; 32]);
        x.copy_from_slice(&plain[1..33]);
        e.copy_from_slice(&plain[33..65]);
        Ok(Self::from_secrets(&x, &e))
    }

    fn sign(&self, msg: &[u8]) -> Signature64 {
        Fixed(self.ed25519.sign(msg).to_bytes())
    }

    /// Signature over `layout::auth_signing_bytes`.
    pub fn sign_auth_challenge(
        &self,
        challenge_id: Uuid,
        device_id: Uuid,
        nonce: &PublicKey32,
    ) -> Signature64 {
        self.sign(&layout::auth_signing_bytes(challenge_id, device_id, nonce))
    }
}

/// A team key `TK_v`. Zeroized on drop; `Debug` is redacted.
pub struct TeamKey {
    key_version: u32,
    bytes: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for TeamKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TeamKey(v{}, <redacted>)", self.key_version)
    }
}

impl TeamKey {
    pub fn generate(key_version: u32) -> Self {
        Self {
            key_version,
            bytes: random_bytes::<32>(),
        }
    }

    pub fn key_version(&self) -> u32 {
        self.key_version
    }

    /// `VK_v = HKDF-SHA256(salt = team_id, ikm = TK_v, info = VAULT_KEY ‖ key_version)`.
    fn vault_key(&self, team_id: Uuid) -> Zeroizing<[u8; 32]> {
        let mut info = layout::kdf::VAULT_KEY.to_vec();
        info.extend_from_slice(&self.key_version.to_be_bytes());
        let hk = Hkdf::<Sha256>::new(Some(team_id.as_bytes()), &self.bytes[..]);
        let mut out = Zeroizing::new([0u8; 32]);
        // 32 bytes is far below the HKDF-SHA256 limit; this cannot fail.
        let _ = hk.expand(&info, &mut out[..]);
        out
    }
}

/// `wrap_key = HKDF(salt = eph_pub ‖ recipient_pub, ikm = shared, info = KDF_KEY_WRAP)`.
fn wrap_key(
    shared: &x25519_dalek::SharedSecret,
    eph_pub: &[u8; 32],
    recipient_pub: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>> {
    if !shared.was_contributory() {
        return Err(TeamsCryptoError::WeakKey);
    }
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(eph_pub);
    salt[32..].copy_from_slice(recipient_pub);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(layout::kdf::KEY_WRAP, &mut out[..])
        .map_err(|_| TeamsCryptoError::Malformed("key derivation"))?;
    Ok(out)
}

/// Wrap `key` to `recipient`, signed by the wrapping (admin) device.
pub fn wrap_team_key(
    team_id: Uuid,
    key: &TeamKey,
    recipient: &DeviceInfo,
    wrapping_device_id: Uuid,
    wrapper: &DeviceKeys,
) -> Result<WrappedKey> {
    let eph = EphemeralSecret::random_from_rng(OsRng);
    let eph_pub = XPublic::from(&eph).to_bytes();
    let recipient_pub = XPublic::from(*recipient.x25519_public.as_bytes());
    let shared = eph.diffie_hellman(&recipient_pub);
    let wk = wrap_key(&shared, &eph_pub, recipient_pub.as_bytes())?;
    drop(shared);

    let aad = layout::key_wrap_aad(
        team_id,
        key.key_version,
        recipient.device_id,
        wrapping_device_id,
    );
    let nonce = random_nonce();
    let ciphertext = aes_encrypt(&wk, &nonce, &key.bytes[..], &aad)?;

    let mut wrapped = WrappedKey {
        team_id,
        key_version: key.key_version,
        recipient_device_id: recipient.device_id,
        wrapping_device_id,
        ephemeral_public: Fixed(eph_pub),
        nonce: Fixed(nonce),
        ciphertext: Bytes(ciphertext),
        signature: Fixed::ZERO,
    };
    wrapped.signature = wrapper.sign(&layout::key_wrap_signing_bytes(&wrapped));
    Ok(wrapped)
}

/// Verify `wrapped` was signed by `trusted_admin` (a pinned Ed25519 key), then
/// unwrap it with this device's X25519 key.
pub fn unwrap_team_key(
    wrapped: &WrappedKey,
    me: &DeviceKeys,
    trusted_admin: &PublicKey32,
) -> Result<TeamKey> {
    if !verify(
        trusted_admin,
        &layout::key_wrap_signing_bytes(wrapped),
        &wrapped.signature,
    ) {
        return Err(TeamsCryptoError::UntrustedSigner);
    }
    let eph_pub = XPublic::from(*wrapped.ephemeral_public.as_bytes());
    let shared = me.x25519.diffie_hellman(&eph_pub);
    let my_pub = XPublic::from(&me.x25519).to_bytes();
    let wk = wrap_key(&shared, eph_pub.as_bytes(), &my_pub)?;
    drop(shared);

    let aad = layout::key_wrap_aad(
        wrapped.team_id,
        wrapped.key_version,
        wrapped.recipient_device_id,
        wrapped.wrapping_device_id,
    );
    let plain = aes_decrypt(&wk, wrapped.nonce.as_bytes(), &wrapped.ciphertext.0, &aad)?;
    if plain.len() != 32 {
        return Err(TeamsCryptoError::Malformed("team key (wrong length)"));
    }
    let mut bytes = Zeroizing::new([0u8; 32]);
    bytes.copy_from_slice(&plain);
    Ok(TeamKey {
        key_version: wrapped.key_version,
        bytes,
    })
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
    let plain = Zeroizing::new(
        serde_json::to_vec(payload).map_err(|_| TeamsCryptoError::Malformed("vault payload"))?,
    );
    let vk = key.vault_key(team_id);
    let aad = layout::entry_aad(
        team_id,
        entry_id,
        base_revision,
        key.key_version,
        author_device_id,
    );
    let nonce = random_nonce();
    let ciphertext = aes_encrypt(&vk, &nonce, &plain, &aad)?;
    let mut entry = EncryptedEntry {
        entry_id,
        revision: 0,
        base_revision,
        key_version: key.key_version,
        author_device_id,
        nonce: Fixed(nonce),
        ciphertext: Bytes(ciphertext),
        signature: Fixed::ZERO,
    };
    entry.signature = author.sign(&layout::entry_signing_bytes(team_id, &entry));
    Ok(entry)
}

/// Verify the author's signature, then decrypt. `key` must match
/// `entry.key_version`.
pub fn open_entry(
    team_id: Uuid,
    key: &TeamKey,
    entry: &EncryptedEntry,
    author_ed25519: &PublicKey32,
) -> Result<VaultPayload> {
    if !verify(
        author_ed25519,
        &layout::entry_signing_bytes(team_id, entry),
        &entry.signature,
    ) {
        return Err(TeamsCryptoError::BadSignature);
    }
    if key.key_version != entry.key_version {
        return Err(TeamsCryptoError::Malformed("entry (key version mismatch)"));
    }
    let vk = key.vault_key(team_id);
    let aad = layout::entry_aad(
        team_id,
        entry.entry_id,
        entry.base_revision,
        entry.key_version,
        entry.author_device_id,
    );
    let plain = aes_decrypt(&vk, entry.nonce.as_bytes(), &entry.ciphertext.0, &aad)?;
    serde_json::from_slice(&plain).map_err(|_| TeamsCryptoError::Malformed("vault payload"))
}

/// Fill in `event.signature` over `layout::audit_signing_bytes`.
pub fn sign_audit_event(event: &mut AuditEvent, keys: &DeviceKeys) {
    event.signature = keys.sign(&layout::audit_signing_bytes(event));
}

pub fn verify_audit_event(event: &AuditEvent, device_ed25519: &PublicKey32) -> Result<()> {
    if verify(
        device_ed25519,
        &layout::audit_signing_bytes(event),
        &event.signature,
    ) {
        Ok(())
    } else {
        Err(TeamsCryptoError::BadSignature)
    }
}

/// OpenSSH-style fingerprint of an Ed25519 key, `SHA256:<base64 no padding>`,
/// shown to admins and pinned by invitees.
pub fn fingerprint(ed25519: &PublicKey32) -> String {
    let algo = b"ssh-ed25519";
    let mut blob = Vec::with_capacity(4 + algo.len() + 4 + 32);
    blob.extend_from_slice(&(algo.len() as u32).to_be_bytes());
    blob.extend_from_slice(algo);
    blob.extend_from_slice(&32u32.to_be_bytes());
    blob.extend_from_slice(ed25519.as_bytes());
    format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(&blob)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::api::{AuditKind, ConnectionRecord};
    use chrono::Utc;

    type Mutation<T> = (&'static str, Box<dyn Fn(&mut T)>);

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    fn payload() -> VaultPayload {
        VaultPayload::Connection(ConnectionRecord {
            name: "prod".into(),
            host: "10.0.0.1".into(),
            user: "root".into(),
            port: 22,
            bastion: None,
            bastion_user: None,
            use_kerberos: false,
            tags: vec!["a".into()],
            notes: Some("n".into()),
        })
    }

    fn info(id: Uuid, keys: &DeviceKeys) -> DeviceInfo {
        DeviceInfo {
            device_id: id,
            account_id: Uuid::nil(),
            name: "d".into(),
            x25519_public: keys.x25519_public(),
            ed25519_public: keys.ed25519_public(),
            created_at: Utc::now(),
            revoked_at: None,
        }
    }

    struct Fx {
        admin: DeviceKeys,
        author: DeviceKeys,
        key: TeamKey,
        entry: EncryptedEntry,
    }

    fn fx() -> Fx {
        let author = DeviceKeys::generate();
        let key = TeamKey::generate(3);
        let entry = seal_entry(id(1), &key, id(2), 5, id(4), &author, &payload()).unwrap();
        Fx {
            admin: DeviceKeys::generate(),
            author,
            key,
            entry,
        }
    }

    fn open(f: &Fx, e: &EncryptedEntry) -> Result<VaultPayload> {
        open_entry(id(1), &f.key, e, &f.author.ed25519_public())
    }

    #[test]
    fn seal_open_round_trip() {
        let f = fx();
        assert_eq!(f.entry.revision, 0);
        assert_eq!(open(&f, &f.entry).unwrap(), payload());
    }

    #[test]
    fn seals_use_fresh_nonces() {
        let f = fx();
        let e2 = seal_entry(id(1), &f.key, id(2), 5, id(4), &f.author, &payload()).unwrap();
        assert_ne!(f.entry.nonce, e2.nonce);
        assert_ne!(f.entry.ciphertext, e2.ciphertext);
    }

    #[test]
    fn entry_tamper_each_field_fails() {
        let f = fx();
        let muts: Vec<Mutation<EncryptedEntry>> = vec![
            ("ciphertext", Box::new(|e| e.ciphertext.0[0] ^= 1)),
            ("nonce", Box::new(|e| e.nonce.0[0] ^= 1)),
            ("base_revision", Box::new(|e| e.base_revision += 1)),
            ("key_version", Box::new(|e| e.key_version += 1)),
            ("author_device_id", Box::new(|e| e.author_device_id = id(9))),
            ("entry_id", Box::new(|e| e.entry_id = id(9))),
            ("signature", Box::new(|e| e.signature.0[0] ^= 1)),
        ];
        for (name, m) in muts {
            let mut e = f.entry.clone();
            m(&mut e);
            assert!(open(&f, &e).is_err(), "tampered {name} was accepted");
        }
        // Server-assigned revision is not signed and must not matter.
        let mut e = f.entry.clone();
        e.revision = 99;
        assert!(open(&f, &e).is_ok());
    }

    #[test]
    fn entry_wrong_team_id_signer_and_key() {
        let f = fx();
        assert!(matches!(
            open_entry(id(9), &f.key, &f.entry, &f.author.ed25519_public()),
            Err(TeamsCryptoError::BadSignature)
        ));
        assert!(matches!(
            open_entry(id(1), &f.key, &f.entry, &f.admin.ed25519_public()),
            Err(TeamsCryptoError::BadSignature)
        ));
        let other = TeamKey::generate(3);
        assert!(matches!(
            open_entry(id(1), &other, &f.entry, &f.author.ed25519_public()),
            Err(TeamsCryptoError::Decrypt)
        ));
        let wrong_version = TeamKey::generate(4);
        assert!(matches!(
            open_entry(id(1), &wrong_version, &f.entry, &f.author.ed25519_public()),
            Err(TeamsCryptoError::Malformed(_))
        ));
    }

    #[test]
    fn entry_signed_with_tampered_ciphertext_by_attacker_fails_decrypt() {
        // A validly signed entry whose ciphertext was swapped for another key's
        // output must still fail the AEAD check.
        let f = fx();
        let other = TeamKey::generate(3);
        let forged = seal_entry(id(1), &other, id(2), 5, id(4), &f.author, &payload()).unwrap();
        assert!(matches!(open(&f, &forged), Err(TeamsCryptoError::Decrypt)));
    }

    fn wrapped_fx() -> (DeviceKeys, DeviceKeys, TeamKey, WrappedKey) {
        let admin = DeviceKeys::generate();
        let me = DeviceKeys::generate();
        let key = TeamKey::generate(2);
        let w = wrap_team_key(id(1), &key, &info(id(5), &me), id(6), &admin).unwrap();
        (admin, me, key, w)
    }

    #[test]
    fn wrap_unwrap_round_trip() {
        let (admin, me, key, w) = wrapped_fx();
        let got = unwrap_team_key(&w, &me, &admin.ed25519_public()).unwrap();
        assert_eq!(got.key_version(), 2);
        // Same key: an entry sealed with one opens with the other.
        let author = DeviceKeys::generate();
        let e = seal_entry(id(1), &key, id(2), 0, id(4), &author, &payload()).unwrap();
        assert_eq!(
            open_entry(id(1), &got, &e, &author.ed25519_public()).unwrap(),
            payload()
        );
    }

    #[test]
    fn wrap_tamper_each_field_fails() {
        let (admin, me, _key, w) = wrapped_fx();
        let muts: Vec<Mutation<WrappedKey>> = vec![
            ("ciphertext", Box::new(|w| w.ciphertext.0[0] ^= 1)),
            ("nonce", Box::new(|w| w.nonce.0[0] ^= 1)),
            ("key_version", Box::new(|w| w.key_version += 1)),
            ("team_id", Box::new(|w| w.team_id = id(9))),
            (
                "recipient_device_id",
                Box::new(|w| w.recipient_device_id = id(9)),
            ),
            (
                "wrapping_device_id",
                Box::new(|w| w.wrapping_device_id = id(9)),
            ),
            (
                "ephemeral_public",
                Box::new(|w| w.ephemeral_public.0[0] ^= 1),
            ),
            ("signature", Box::new(|w| w.signature.0[0] ^= 1)),
        ];
        for (name, m) in muts {
            let mut t = w.clone();
            m(&mut t);
            assert!(
                unwrap_team_key(&t, &me, &admin.ed25519_public()).is_err(),
                "tampered {name} was accepted"
            );
        }
    }

    #[test]
    fn aead_binds_wrap_aad_even_if_resigned() {
        // A pinned admin re-signing altered ids still can't make the AEAD pass.
        let (admin, me, _key, w) = wrapped_fx();
        let mut t = w.clone();
        t.recipient_device_id = id(9);
        t.signature = admin.sign(&layout::key_wrap_signing_bytes(&t));
        assert!(matches!(
            unwrap_team_key(&t, &me, &admin.ed25519_public()),
            Err(TeamsCryptoError::Decrypt)
        ));
    }

    #[test]
    fn unwrap_rejects_untrusted_signer_and_wrong_recipient() {
        let (_admin, me, _key, w) = wrapped_fx();
        let other = DeviceKeys::generate();
        assert!(matches!(
            unwrap_team_key(&w, &me, &other.ed25519_public()),
            Err(TeamsCryptoError::UntrustedSigner)
        ));
        let (admin, _me, _k, w2) = wrapped_fx();
        let stranger = DeviceKeys::generate();
        assert!(matches!(
            unwrap_team_key(&w2, &stranger, &admin.ed25519_public()),
            Err(TeamsCryptoError::Decrypt)
        ));
        // Garbage pinned key is untrusted, not a panic.
        assert!(matches!(
            unwrap_team_key(&w, &me, &Fixed([0xff; 32])),
            Err(TeamsCryptoError::UntrustedSigner)
        ));
    }

    #[test]
    fn unwrap_requires_32_byte_plaintext() {
        let (admin, me, _key, w) = wrapped_fx();
        let eph_pub = XPublic::from(*w.ephemeral_public.as_bytes());
        let shared = me.x25519.diffie_hellman(&eph_pub);
        let wk = wrap_key(&shared, eph_pub.as_bytes(), &me.x25519_public().0).unwrap();
        let aad = layout::key_wrap_aad(
            w.team_id,
            w.key_version,
            w.recipient_device_id,
            w.wrapping_device_id,
        );
        let mut t = w.clone();
        t.ciphertext = Bytes(aes_encrypt(&wk, &t.nonce.0, &[7u8; 31], &aad).unwrap());
        t.signature = admin.sign(&layout::key_wrap_signing_bytes(&t));
        assert!(matches!(
            unwrap_team_key(&t, &me, &admin.ed25519_public()),
            Err(TeamsCryptoError::Malformed(_))
        ));
    }

    #[test]
    fn low_order_recipient_is_weak_key() {
        let admin = DeviceKeys::generate();
        let mut recipient = info(id(5), &DeviceKeys::generate());
        recipient.x25519_public = Fixed([0u8; 32]);
        assert!(matches!(
            wrap_team_key(id(1), &TeamKey::generate(1), &recipient, id(6), &admin),
            Err(TeamsCryptoError::WeakKey)
        ));
        // Low-order ephemeral point on the unwrap side.
        let (admin, me, _k, w) = wrapped_fx();
        let mut t = w.clone();
        t.ephemeral_public = Fixed([0u8; 32]);
        t.signature = admin.sign(&layout::key_wrap_signing_bytes(&t));
        assert!(matches!(
            unwrap_team_key(&t, &me, &admin.ed25519_public()),
            Err(TeamsCryptoError::WeakKey)
        ));
    }

    #[test]
    fn device_keys_seal_unseal() {
        let k = DeviceKeys::generate();
        let sealed = k.seal("correct horse").unwrap();
        let back = DeviceKeys::unseal(&sealed, "correct horse").unwrap();
        assert_eq!(back.ed25519_public(), k.ed25519_public());
        assert_eq!(back.x25519_public(), k.x25519_public());
        assert!(matches!(
            DeviceKeys::unseal(&sealed, "wrong"),
            Err(TeamsCryptoError::Sealed(_))
        ));
        assert!(matches!(
            DeviceKeys::unseal(b"junk", "correct horse"),
            Err(TeamsCryptoError::Sealed(_))
        ));
        assert_eq!(format!("{k:?}"), "DeviceKeys(<redacted>)");
        let nd = k.to_new_device("laptop");
        assert_eq!(nd.name, "laptop");
        assert_eq!(nd.ed25519_public, k.ed25519_public());
    }

    #[test]
    fn auth_challenge_signature_verifies() {
        let k = DeviceKeys::generate();
        let nonce = Fixed([3u8; 32]);
        let sig = k.sign_auth_challenge(id(1), id(2), &nonce);
        let bytes = layout::auth_signing_bytes(id(1), id(2), &nonce);
        assert!(verify(&k.ed25519_public(), &bytes, &sig));
        assert!(!verify(
            &k.ed25519_public(),
            &layout::auth_signing_bytes(id(1), id(3), &nonce),
            &sig
        ));
    }

    fn event() -> AuditEvent {
        AuditEvent {
            event_id: id(6),
            team_id: id(1),
            device_id: id(4),
            seq: 0,
            kind: AuditKind::VaultRead,
            entry_id: Some(id(2)),
            occurred_at: Utc::now(),
            prev_hash: layout::AUDIT_GENESIS,
            signature: Fixed::ZERO,
        }
    }

    #[test]
    fn audit_sign_verify_and_tamper() {
        let k = DeviceKeys::generate();
        let mut ev = event();
        sign_audit_event(&mut ev, &k);
        assert!(verify_audit_event(&ev, &k.ed25519_public()).is_ok());
        let mut t = ev.clone();
        t.seq = 1;
        assert!(matches!(
            verify_audit_event(&t, &k.ed25519_public()),
            Err(TeamsCryptoError::BadSignature)
        ));
        let mut t = ev.clone();
        t.signature.0[5] ^= 1;
        assert!(verify_audit_event(&t, &k.ed25519_public()).is_err());
        assert!(verify_audit_event(&ev, &DeviceKeys::generate().ed25519_public()).is_err());
    }

    #[test]
    fn fingerprint_matches_ssh_keygen() {
        // Throwaway key made with `ssh-keygen -t ed25519 -N ''`:
        //   ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAID2tB37Du9D8F2MgJ+MhjpVxo1t0BuUceCkF/3ngwcQS
        //   256 SHA256:SF9WlmfWuKAxL+IE2QnAygAmBz78F4TZDzR549n0G5Q (ED25519)
        let raw = "3dad077ec3bbd0fc17632027e3218e9571a35b7406e51c782905ff79e0c1c412";
        let mut pk = [0u8; 32];
        for (i, b) in pk.iter_mut().enumerate() {
            *b = u8::from_str_radix(&raw[2 * i..2 * i + 2], 16).unwrap();
        }
        assert_eq!(
            fingerprint(&Fixed(pk)),
            "SHA256:SF9WlmfWuKAxL+IE2QnAygAmBz78F4TZDzR549n0G5Q"
        );
    }
}
