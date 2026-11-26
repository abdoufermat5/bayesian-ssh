//! Binary field types. Everything binary travels as standard base64 (with
//! padding); fixed-size fields reject the wrong length at deserialization.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

/// A fixed-length byte string (keys, nonces, signatures, hashes).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fixed<const N: usize>(pub [u8; N]);

pub type PublicKey32 = Fixed<32>;
pub type Signature64 = Fixed<64>;
pub type Nonce12 = Fixed<12>;
pub type Hash32 = Fixed<32>;

impl<const N: usize> Fixed<N> {
    pub const ZERO: Self = Fixed([0u8; N]);

    pub fn as_bytes(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> From<[u8; N]> for Fixed<N> {
    fn from(b: [u8; N]) -> Self {
        Fixed(b)
    }
}

impl<const N: usize> fmt::Debug for Fixed<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed<{N}>({})", STANDARD.encode(self.0))
    }
}

impl<const N: usize> Serialize for Fixed<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(self.0))
    }
}

impl<'de, const N: usize> Deserialize<'de> for Fixed<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct FixedVisitor<const N: usize>;

        impl<const N: usize> de::Visitor<'_> for FixedVisitor<N> {
            type Value = Fixed<N>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a standard base64 string encoding exactly {N} bytes")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let expected_b64_len = N.div_ceil(3) * 4;
                if v.len() != expected_b64_len {
                    return Err(de::Error::invalid_length(
                        v.len(),
                        &format!("{expected_b64_len} base64 characters ({N} bytes)").as_str(),
                    ));
                }
                let mut buf = [0u8; N];
                let decoded = STANDARD
                    .decode_slice(v.as_bytes(), &mut buf)
                    .map_err(de::Error::custom)?;
                if decoded != N {
                    return Err(de::Error::invalid_length(
                        decoded,
                        &format!("{N} bytes").as_str(),
                    ));
                }
                Ok(Fixed(buf))
            }
        }

        d.deserialize_str(FixedVisitor::<N>)
    }
}

/// A variable-length byte string (ciphertexts).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({} bytes)", self.0.len())
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct BytesVisitor;

        impl de::Visitor<'_> for BytesVisitor {
            type Value = Bytes;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a standard base64 string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                STANDARD
                    .decode(v.as_bytes())
                    .map(Bytes)
                    .map_err(de::Error::custom)
            }
        }

        d.deserialize_str(BytesVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_round_trips_as_base64() {
        let k = Fixed([7u8; 32]);
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, "\"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=\"");
        assert_eq!(serde_json::from_str::<Fixed<32>>(&json).unwrap(), k);

        let n = Fixed([3u8; 12]);
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(serde_json::from_str::<Fixed<12>>(&json).unwrap(), n);

        let s = Fixed([9u8; 64]);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Fixed<64>>(&json).unwrap(), s);

        let empty = Fixed([]);
        let json = serde_json::to_string(&empty).unwrap();
        assert_eq!(json, "\"\"");
        assert_eq!(serde_json::from_str::<Fixed<0>>(&json).unwrap(), empty);
    }

    #[test]
    fn fixed_rejects_wrong_length() {
        let short = serde_json::to_string(&Bytes(vec![1; 31])).unwrap();
        assert!(serde_json::from_str::<Fixed<32>>(&short).is_err());

        // 33 bytes encodes to 44 base64 chars (same b64 length as 32 bytes, no padding)
        let one_over = serde_json::to_string(&Bytes(vec![1; 33])).unwrap();
        assert!(serde_json::from_str::<Fixed<32>>(&one_over).is_err());

        // Huge oversized payload is rejected before decoding
        let huge = format!("\"{}\"", "A".repeat(1024));
        assert!(serde_json::from_str::<Fixed<32>>(&huge).is_err());
    }

    #[test]
    fn fixed_rejects_malformed_and_unpadded_base64() {
        // Right length (44 chars) but invalid base64 characters
        let malformed = format!("\"{}\"", "!".repeat(44));
        assert!(serde_json::from_str::<Fixed<32>>(&malformed).is_err());

        // Unpadded base64 for 32 bytes (43 chars instead of 44)
        let unpadded = "\"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc\"";
        assert!(serde_json::from_str::<Fixed<32>>(unpadded).is_err());
    }

    #[test]
    fn bytes_round_trips_and_rejects_malformed() {
        let b = Bytes(b"hello world\x00\xff".to_vec());
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<Bytes>(&json).unwrap(), b);

        let empty = Bytes(Vec::new());
        let json = serde_json::to_string(&empty).unwrap();
        assert_eq!(json, "\"\"");
        assert_eq!(serde_json::from_str::<Bytes>(&json).unwrap(), empty);

        assert!(serde_json::from_str::<Bytes>("\"not!valid!b64\"").is_err());
    }
}
