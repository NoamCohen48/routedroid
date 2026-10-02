//! Serde adapter for the 32-byte hex fields (nonces and proofs): exactly 64
//! lowercase hex characters on the wire, `[u8; 32]` in memory, so a decoded
//! body cannot hold a value that still needs checking.

use serde::de::{Deserializer, Error, Visitor};
use serde::Serializer;

pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(bytes))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
    d.deserialize_str(Hex32)
}

struct Hex32;

impl Visitor<'_> for Hex32 {
    type Value = [u8; 32];

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("64 lowercase hex characters")
    }

    fn visit_str<E: Error>(self, v: &str) -> Result<Self::Value, E> {
        let lowercase = v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        let mut out = [0u8; 32];
        match hex::decode_to_slice(v, &mut out) {
            Ok(()) if lowercase => Ok(out),
            _ => Err(E::invalid_value(serde::de::Unexpected::Str(v), &self)),
        }
    }
}
