//! Binary field types. Everything binary travels as standard base64 (with
//! padding); fixed-size fields reject the wrong length at deserialization.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
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
        let s = String::deserialize(d)?;
        let v = STANDARD.decode(s.as_bytes()).map_err(de::Error::custom)?;
        let arr: [u8; N] = v
            .try_into()
            .map_err(|v: Vec<u8>| de::Error::invalid_length(v.len(), &&*format!("{N} bytes")))?;
        Ok(Fixed(arr))
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
        let s = String::deserialize(d)?;
        STANDARD
            .decode(s.as_bytes())
            .map(Bytes)
            .map_err(de::Error::custom)
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
    }

    #[test]
    fn fixed_rejects_wrong_length() {
        let short = serde_json::to_string(&Bytes(vec![1; 31])).unwrap();
        assert!(serde_json::from_str::<Fixed<32>>(&short).is_err());
    }
}
