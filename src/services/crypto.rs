//! Crypto service module providing PBKDF2-HMAC-SHA256 key derivation
//! and authenticated payload encryption/decryption for secure backups.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const MAGIC: &[u8; 4] = b"BSSH";
const SALT_LEN: usize = 16;
const IV_LEN: usize = 16;
const HMAC_LEN: usize = 32;
const PBKDF2_ROUNDS: u32 = 600_000;
const LEGACY_PBKDF2_ROUNDS: u32 = 10_000;

/// Derive a 64-byte key (32 bytes encryption key + 32 bytes HMAC key) using PBKDF2-HMAC-SHA256.
fn derive_keys(passphrase: &str, salt: &[u8], rounds: u32) -> (Vec<u8>, Vec<u8>) {
    let mut derived = vec![0u8; 64];
    pbkdf2_sha256(passphrase.as_bytes(), salt, rounds, &mut derived);
    (derived[0..32].to_vec(), derived[32..64].to_vec())
}

/// Simple implementation of PBKDF2 with HMAC-SHA256
fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
    let block_count = out.len().div_ceil(32);
    for i in 1..=block_count {
        let mut mac = HmacSha256::new_from_slice(password).expect("HMAC can take any key length");
        mac.update(salt);
        mac.update(&(i as u32).to_be_bytes());
        let mut u = mac.finalize().into_bytes();
        let mut t = u;

        for _ in 1..rounds {
            let mut mac =
                HmacSha256::new_from_slice(password).expect("HMAC can take any key length");
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

/// Stream cipher keystream generator using AES/SHA256 CTR mode from derived key + IV
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

/// Encrypt arbitrary data using passphrase-derived keys and Encrypt-then-MAC authentication.
pub fn encrypt_data(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut iv = [0u8; IV_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut iv);

    let (enc_key, mac_key) = derive_keys(passphrase, &salt, PBKDF2_ROUNDS);
    let ks = keystream(&enc_key, &iv, data.len());

    let mut ciphertext = vec![0u8; data.len()];
    for i in 0..data.len() {
        ciphertext[i] = data[i] ^ ks[i];
    }

    let mut mac = HmacSha256::new_from_slice(&mac_key)?;
    mac.update(MAGIC);
    mac.update(&salt);
    mac.update(&iv);
    mac.update(&ciphertext);
    let mac_tag = mac.finalize().into_bytes();

    let mut output =
        Vec::with_capacity(MAGIC.len() + SALT_LEN + IV_LEN + HMAC_LEN + ciphertext.len());
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&salt);
    output.extend_from_slice(&iv);
    output.extend_from_slice(&mac_tag);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// Decrypt encrypted data using passphrase.
pub fn decrypt_data(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let min_len = MAGIC.len() + SALT_LEN + IV_LEN + HMAC_LEN;
    if encrypted.len() < min_len {
        return Err(anyhow!("Invalid encrypted payload: file too short"));
    }

    if &encrypted[..4] != MAGIC {
        return Err(anyhow!(
            "Invalid encrypted file format: missing BSSH header"
        ));
    }

    let salt = &encrypted[4..20];
    let iv = &encrypted[20..36];
    let expected_mac = &encrypted[36..68];
    let ciphertext = &encrypted[68..];

    // Try current 600,000 rounds first, fall back to legacy 10,000 rounds if MAC check fails
    for &rounds in &[PBKDF2_ROUNDS, LEGACY_PBKDF2_ROUNDS] {
        let (enc_key, mac_key) = derive_keys(passphrase, salt, rounds);

        let mut mac = match HmacSha256::new_from_slice(&mac_key) {
            Ok(m) => m,
            Err(_) => continue,
        };
        mac.update(MAGIC);
        mac.update(salt);
        mac.update(iv);
        mac.update(ciphertext);

        if mac.verify_slice(expected_mac).is_ok() {
            let ks = keystream(&enc_key, iv, ciphertext.len());
            let mut plaintext = vec![0u8; ciphertext.len()];
            for i in 0..ciphertext.len() {
                plaintext[i] = ciphertext[i] ^ ks[i];
            }
            return Ok(plaintext);
        }
    }

    Err(anyhow!(
        "Decryption failed: invalid passphrase or corrupted file"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let payload = b"Hello, Bayesian SSH encrypted backup!";
        let passphrase = "SuperSecretPassphrase123!";

        let encrypted = encrypt_data(payload, passphrase).expect("encryption succeeds");
        assert_ne!(encrypted, payload);

        let decrypted = decrypt_data(&encrypted, passphrase).expect("decryption succeeds");
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let payload = b"Secret connection settings";
        let encrypted = encrypt_data(payload, "correct_pass").unwrap();
        let result = decrypt_data(&encrypted, "wrong_pass");
        assert!(result.is_err());
    }
}
