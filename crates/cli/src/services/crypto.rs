//! Authenticated encryption service for secure backups and exports.
//!
//! Construction: AES-256-GCM with a key derived from a passphrase via
//! Argon2id. Each encrypted payload carries a version byte so that future
//! format migrations can coexist with current readers.
//!
//! ## On-disk format (v2)
//!
//! ```text
//! magic[4]   = b"BSSH"        (legacy marker; kept for backward compat with v1)
//! version[1] = 0x02
//! salt[16]   (Argon2id salt)
//! nonce[12]  (AES-GCM nonce; unique per call via OsRng)
//! ciphertext[..]  (AES-256-GCM ciphertext + 16-byte GCM tag, appended)
//! ```
//!
//! ## v1 (legacy) detection
//!
//! v1 payloads start with the `BSSH` magic, but the byte immediately after
//! the magic is the PBKDF2 iteration count prefix (`0x01` = v1) rather than
//! `0x02`. v1 was a custom SHA-256 stream cipher and is no longer
//! recommended; the current code can still read v1 payloads for backward
//! compatibility but always writes v2.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Result};
use argon2::Algorithm::Argon2id;
use argon2::{Params, Version};
use rand::RngCore;
use zeroize::Zeroize;

const MAGIC: &[u8; 4] = b"BSSH";
const VERSION_V2: u8 = 0x02;
const VERSION_V1: u8 = 0x01;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

const ARGON2_MEM_KIB: u32 = 64 * 1024;
const ARGON2_TIME_COST: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;

fn derive_key_argon2(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let params = Params::new(
        ARGON2_MEM_KIB,
        ARGON2_TIME_COST,
        ARGON2_PARALLELISM,
        Some(KEY_LEN),
    )
    .map_err(|e| anyhow!("invalid Argon2 parameters: {e}"))?;
    let argon = argon2::Argon2::new(Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("Argon2id key derivation failed: {e}"))?;
    Ok(key)
}

fn aad(version: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(MAGIC.len() + 1);
    v.extend_from_slice(MAGIC);
    v.push(version);
    v
}

/// Encrypt arbitrary data using a passphrase-derived key (AES-256-GCM,
/// Argon2id KDF, v2 format).
pub fn encrypt_data(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let mut key_bytes = derive_key_argon2(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let aad = aad(VERSION_V2);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: data,
                aad: &aad,
            },
        )
        .map_err(|e| anyhow!("AES-GCM encryption failed: {e}"))?;
    key_bytes.zeroize();

    let mut out = Vec::with_capacity(MAGIC.len() + 1 + SALT_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION_V2);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a payload produced by [`encrypt_data`] (v2) or by the legacy
/// v1 stream-cipher format. Returns an error on tampered ciphertext,
/// wrong passphrase, or unrecognised format.
pub fn decrypt_data(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let header_len = MAGIC.len() + 1;
    if encrypted.len() < header_len {
        bail!("Invalid encrypted payload: file too short");
    }
    if &encrypted[..MAGIC.len()] != MAGIC {
        bail!("Invalid encrypted file format: missing BSSH header");
    }

    let version = encrypted[MAGIC.len()];
    match version {
        VERSION_V2 => decrypt_v2(encrypted, passphrase),
        VERSION_V1 => decrypt_v1_legacy(encrypted, passphrase),
        other => bail!("Unsupported encrypted payload version: 0x{other:02x}"),
    }
}

fn decrypt_v2(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let offset = MAGIC.len() + 1;
    if encrypted.len() < offset + SALT_LEN + NONCE_LEN + 16 {
        bail!("Invalid v2 payload: too short");
    }
    let salt = &encrypted[offset..offset + SALT_LEN];
    let nonce = &encrypted[offset + SALT_LEN..offset + SALT_LEN + NONCE_LEN];
    let ciphertext = &encrypted[offset + SALT_LEN + NONCE_LEN..];

    let mut key_bytes = derive_key_argon2(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let aad = aad(VERSION_V2);
    let pt = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow!("Decryption failed: invalid passphrase or corrupted file"));
    key_bytes.zeroize();
    pt
}

// ──────────────────────────────────────────────────────────────────────────────
// v1 (legacy) stream-cipher reader — used only for backward-compat decryption
// of pre-2.6.0 backups. New code should always produce v2.
// ──────────────────────────────────────────────────────────────────────────────

mod v1_legacy {
    use anyhow::{bail, Result};
    use hmac::Mac;
    use sha2::{Digest, Sha256};
    type HmacSha256 = hmac::Hmac<Sha256>;

    const SALT_LEN: usize = 16;
    const IV_LEN: usize = 16;
    const HMAC_LEN: usize = 32;
    pub const PBKDF2_ROUNDS: u32 = 600_000;
    pub const LEGACY_PBKDF2_ROUNDS: u32 = 10_000;

    pub fn derive_keys(passphrase: &str, salt: &[u8], rounds: u32) -> (Vec<u8>, Vec<u8>) {
        let mut derived = vec![0u8; 64];
        pbkdf2_sha256(passphrase.as_bytes(), salt, rounds, &mut derived);
        (derived[0..32].to_vec(), derived[32..64].to_vec())
    }

    fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
        let block_count = out.len().div_ceil(32);
        for i in 1..=block_count {
            let mut mac = <HmacSha256 as Mac>::new_from_slice(password)
                .expect("HMAC can take any key length");
            mac.update(salt);
            mac.update(&(i as u32).to_be_bytes());
            let mut u = mac.finalize().into_bytes();
            let mut t = u;
            for _ in 1..rounds {
                let mut mac = <HmacSha256 as Mac>::new_from_slice(password)
                    .expect("HMAC can take any key length");
                mac.update(&u);
                u = mac.finalize().into_bytes();
                for (tb, ub) in t.iter_mut().zip(u.iter()) {
                    *tb ^= *ub;
                }
            }
            let start = (i - 1) * 32;
            let end = (start + 32).min(out.len());
            out[start..end].copy_from_slice(&t[..end - start]);
        }
    }

    fn keystream(key: &[u8], iv: &[u8], length: usize) -> Vec<u8> {
        let mut stream = Vec::with_capacity(length);
        let mut counter = 0u64;
        while stream.len() < length {
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.update(iv);
            hasher.update(counter.to_be_bytes());
            let block = hasher.finalize();
            stream.extend_from_slice(&block);
            counter += 1;
        }
        stream.truncate(length);
        stream
    }

    pub fn try_decrypt(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>> {
        let header_len = super::MAGIC.len() + 1 + SALT_LEN + IV_LEN + HMAC_LEN;
        if encrypted.len() < header_len {
            bail!("v1 payload too short");
        }
        let salt = &encrypted[super::MAGIC.len() + 1..super::MAGIC.len() + 1 + SALT_LEN];
        let iv = &encrypted
            [super::MAGIC.len() + 1 + SALT_LEN..super::MAGIC.len() + 1 + SALT_LEN + IV_LEN];
        let expected_mac = &encrypted[super::MAGIC.len() + 1 + SALT_LEN + IV_LEN
            ..super::MAGIC.len() + 1 + SALT_LEN + IV_LEN + HMAC_LEN];
        let ciphertext = &encrypted[super::MAGIC.len() + 1 + SALT_LEN + IV_LEN + HMAC_LEN..];

        for &rounds in &[PBKDF2_ROUNDS, LEGACY_PBKDF2_ROUNDS] {
            let (enc_key, mac_key) = derive_keys(passphrase, salt, rounds);
            let mut mac = match <HmacSha256 as Mac>::new_from_slice(&mac_key) {
                Ok(m) => m,
                Err(_) => continue,
            };
            mac.update(super::MAGIC);
            mac.update(&[super::VERSION_V1]);
            mac.update(salt);
            mac.update(iv);
            mac.update(ciphertext);
            if mac.verify_slice(expected_mac).is_ok() {
                let ks = keystream(&enc_key, iv, ciphertext.len());
                let mut pt = vec![0u8; ciphertext.len()];
                for i in 0..ciphertext.len() {
                    pt[i] = ciphertext[i] ^ ks[i];
                }
                return Ok(pt);
            }
        }
        bail!("Decryption failed: invalid passphrase or corrupted file")
    }
}

fn decrypt_v1_legacy(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    // Skip the magic + version byte; the v1 reader expects the salt at the
    // position after the version byte, which is the format we wrote.
    v1_legacy::try_decrypt(encrypted, passphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let payload = b"Hello, Bayesian SSH encrypted backup!";
        let passphrase = "SuperSecretPassphrase123!";

        let encrypted = encrypt_data(payload, passphrase).expect("encryption succeeds");
        assert_ne!(encrypted, payload);
        assert_eq!(&encrypted[..MAGIC.len()], MAGIC);
        assert_eq!(encrypted[MAGIC.len()], VERSION_V2);

        let decrypted = decrypt_data(&encrypted, passphrase).expect("decryption succeeds");
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let payload = b"Secret connection settings";
        let encrypted = encrypt_data(payload, "correct_pass").unwrap();
        let result = decrypt_data(&encrypted, "wrong_pass");
        assert!(result.is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let payload = b"Secret connection settings";
        let mut encrypted = encrypt_data(payload, "pass").unwrap();
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;
        let result = decrypt_data(&encrypted, "pass");
        assert!(result.is_err());
    }

    #[test]
    fn tampered_aad_fails() {
        let payload = b"Secret connection settings";
        let mut encrypted = encrypt_data(payload, "pass").unwrap();
        // Flip the version byte (which is part of the AAD).
        encrypted[MAGIC.len()] = 0x03;
        let result = decrypt_data(&encrypted, "pass");
        assert!(result.is_err());
    }

    #[test]
    fn v1_legacy_still_decrypts() {
        // Synthesise a v1 payload via the legacy reader and ensure v2 reader
        // can still decrypt it. We exercise the v1 path by feeding a small
        // fixture that satisfies the v1 header layout.
        //
        // A v1 payload is: magic(4) | 0x01 | salt(16) | iv(16) | mac(32) | ct
        // We'll encrypt with the v1 helper, then decrypt via the public API.
        use ::hmac::Mac;
        use sha2::{Digest, Sha256};
        type HmacSha256 = ::hmac::Hmac<Sha256>;

        let mut salt = [0u8; 16];
        let mut iv = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        rand::rngs::OsRng.fill_bytes(&mut iv);

        let passphrase = "legacy-pass";
        let (enc_key, mac_key) = v1_legacy::derive_keys(passphrase, &salt, 10_000);
        let plaintext = b"legacy payload";

        // v1 keystream
        let mut stream = Vec::with_capacity(plaintext.len());
        let mut counter = 0u64;
        while stream.len() < plaintext.len() {
            let mut h = Sha256::new();
            h.update(&enc_key);
            h.update(&iv);
            h.update(counter.to_be_bytes());
            stream.extend_from_slice(&h.finalize());
            counter += 1;
        }
        stream.truncate(plaintext.len());
        let mut ct = plaintext.to_vec();
        for (a, b) in ct.iter_mut().zip(stream.iter()) {
            *a ^= *b;
        }
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&mac_key).unwrap();
        mac.update(MAGIC);
        mac.update(&[VERSION_V1]);
        mac.update(&salt);
        mac.update(&iv);
        mac.update(&ct);
        let tag = mac.finalize().into_bytes();

        let mut v1 = Vec::new();
        v1.extend_from_slice(MAGIC);
        v1.push(VERSION_V1);
        v1.extend_from_slice(&salt);
        v1.extend_from_slice(&iv);
        v1.extend_from_slice(&tag);
        v1.extend_from_slice(&ct);

        let decrypted = decrypt_data(&v1, passphrase).expect("v1 decrypts");
        assert_eq!(decrypted, plaintext);
    }
}
