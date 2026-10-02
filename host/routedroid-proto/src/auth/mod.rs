//! Mutual proof (§7.3).
//!
//! ```text
//! transcript    = "routedroid-auth-v1" 0x00 | protocol u8 | session utf8 | 0x00
//!               | device_port u16be | "android" | client_nonce[32] | "host" | host_nonce[32]
//! host_proof    = HMAC-SHA256(secret, "host"    || transcript)
//! android_proof = HMAC-SHA256(secret, "android" || transcript)
//! ```
//!
//! The secret is zeroized on drop and never printed. Verification is
//! constant-time.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::PROTOCOL_VERSION;

pub const DOMAIN: &[u8] = b"routedroid-auth-v1";
pub const SECRET_LEN: usize = 32;
pub const NONCE_LEN: usize = 32;
pub const PROOF_LEN: usize = 32;

pub type Nonce = [u8; NONCE_LEN];
pub type Proof = [u8; PROOF_LEN];

pub use crate::Role;

fn label(role: Role) -> &'static [u8] {
    match role {
        Role::Host => b"host",
        Role::Android => b"android",
    }
}

/// A session secret: zeroized on drop, never printed, never copied, and
/// compared only through [`verify`]'s constant-time check.
pub struct Secret(Zeroizing<[u8; SECRET_LEN]>);

impl Secret {
    pub fn new(bytes: [u8; SECRET_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn random() -> Result<Self, getrandom::Error> {
        Ok(Self::new(random_bytes()?))
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let v = hex::decode(s.trim()).ok()?;
        let arr: [u8; SECRET_LEN] = v.try_into().ok()?;
        Some(Self::new(arr))
    }

    pub fn as_bytes(&self) -> &[u8; SECRET_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], getrandom::Error> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b)?;
    Ok(b)
}

pub fn random_nonce() -> Result<Nonce, getrandom::Error> {
    random_bytes()
}

pub fn nonce_from_hex(s: &str) -> Option<Nonce> {
    hex::decode(s).ok()?.try_into().ok()
}

pub fn proof_from_hex(s: &str) -> Option<Proof> {
    hex::decode(s).ok()?.try_into().ok()
}

/// The bytes both proofs are computed over.
pub fn transcript(
    session: &str,
    device_port: u16,
    client_nonce: &Nonce,
    host_nonce: &Nonce,
) -> Vec<u8> {
    let mut t = Vec::with_capacity(
        DOMAIN.len() + 1 + 1 + session.len() + 1 + 2 + 7 + NONCE_LEN + 4 + NONCE_LEN,
    );
    t.extend_from_slice(DOMAIN);
    t.push(0);
    t.push(PROTOCOL_VERSION);
    t.extend_from_slice(session.as_bytes());
    t.push(0);
    t.extend_from_slice(&device_port.to_be_bytes());
    t.extend_from_slice(b"android");
    t.extend_from_slice(client_nonce);
    t.extend_from_slice(b"host");
    t.extend_from_slice(host_nonce);
    t
}

pub fn proof(secret: &Secret, role: Role, transcript: &[u8]) -> Proof {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(label(role));
    mac.update(transcript);
    mac.finalize().into_bytes().into()
}

/// Constant-time check of a received proof.
pub fn verify(secret: &Secret, role: Role, transcript: &[u8], received: &Proof) -> bool {
    proof(secret, role, transcript).ct_eq(received).into()
}

#[cfg(test)]
mod tests;
