//! Phase 0 §3.4 mutual authentication (architecture §8.2) and the bootstrap
//! record streamed to Android through `adb shell content write`.
//!
//! ```text
//! transcript = "rd-p0-auth" 0x00 | protocol u8 | session utf8 | 0x00 | device_port u16be
//!            | "android" | client_nonce[32] | "host" | host_nonce[32]
//! host_proof    = HMAC-SHA256(secret, "host"    | transcript)
//! android_proof = HMAC-SHA256(secret, "android" | transcript)
//! ```
//!
//! The secret never appears in a command line or a log line: it is written
//! to the provider's stdin as part of a fixed-size record and zeroized here
//! once the session is authenticated (or failed).

use anyhow::{bail, Context, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

pub const SECRET_LEN: usize = 32;
pub const NONCE_LEN: usize = 32;
pub const PROOF_LEN: usize = 32;

/// Bootstrap record layout (80 bytes, fixed):
/// `magic "RDB0"[4] | version u8 = 0 | reserved[3] | session[40] NUL-padded | secret[32]`.
pub const RECORD_LEN: usize = 80;
pub const RECORD_MAGIC: &[u8; 4] = b"RDB0";
pub const RECORD_VERSION: u8 = 0;
pub const RECORD_SESSION_LEN: usize = 40;

/// A session secret: zeroized on drop, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<[u8; SECRET_LEN]>);

impl Secret {
    pub fn new(bytes: [u8; SECRET_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}

impl std::ops::Deref for Secret {
    type Target = [u8; SECRET_LEN];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

type HmacSha256 = Hmac<Sha256>;

pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).context("getrandom")?;
    Ok(b)
}

pub fn random_secret() -> Result<Secret> {
    Ok(Secret::new(random_bytes::<SECRET_LEN>()?))
}

/// Parse 64 hex characters (surrounding whitespace ignored) into a secret.
pub fn secret_from_hex(text: &str) -> Result<Secret> {
    let raw = hex::decode(text.trim()).context("secret is not hex")?;
    let arr: [u8; SECRET_LEN] =
        raw.as_slice().try_into().map_err(|_| anyhow::anyhow!("secret must be {SECRET_LEN} bytes"))?;
    Ok(Secret::new(arr))
}

pub fn nonce_from_hex(text: &str) -> Option<[u8; NONCE_LEN]> {
    hex::decode(text).ok()?.as_slice().try_into().ok()
}

pub fn transcript(
    protocol: u8,
    session: &str,
    device_port: u16,
    client_nonce: &[u8; NONCE_LEN],
    host_nonce: &[u8; NONCE_LEN],
) -> Vec<u8> {
    let mut t = Vec::with_capacity(64 + session.len() + 2 * NONCE_LEN);
    t.extend_from_slice(b"rd-p0-auth\0");
    t.push(protocol);
    t.extend_from_slice(session.as_bytes());
    t.push(0);
    t.extend_from_slice(&device_port.to_be_bytes());
    t.extend_from_slice(b"android");
    t.extend_from_slice(client_nonce);
    t.extend_from_slice(b"host");
    t.extend_from_slice(host_nonce);
    t
}

pub fn proof(secret: &[u8; SECRET_LEN], role: &str, transcript: &[u8]) -> [u8; PROOF_LEN] {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(role.as_bytes());
    mac.update(transcript);
    mac.finalize().into_bytes().into()
}

/// Constant-time check of a hex-encoded proof.
pub fn verify(secret: &[u8; SECRET_LEN], role: &str, transcript: &[u8], proof_hex: &str) -> bool {
    let Ok(raw) = hex::decode(proof_hex) else { return false };
    if raw.len() != PROOF_LEN {
        return false;
    }
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(role.as_bytes());
    mac.update(transcript);
    mac.verify_slice(&raw).is_ok()
}

/// Build the fixed-size record the host streams to the bootstrap provider.
pub fn bootstrap_record(session: &str, secret: &[u8; SECRET_LEN]) -> Result<Zeroizing<[u8; RECORD_LEN]>> {
    let s = session.as_bytes();
    if s.is_empty() || s.len() > RECORD_SESSION_LEN || s.contains(&0) {
        bail!("session id must be 1..={RECORD_SESSION_LEN} bytes without NUL");
    }
    let mut r = Zeroizing::new([0u8; RECORD_LEN]);
    r[0..4].copy_from_slice(RECORD_MAGIC);
    r[4] = RECORD_VERSION;
    r[8..8 + s.len()].copy_from_slice(s);
    r[48..80].copy_from_slice(secret);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared test vector; the Kotlin unit test asserts the same values.
    pub const TV_SECRET: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11,
        0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    ];

    #[test]
    fn test_vector_matches_kotlin() {
        let client = [0xaa; 32];
        let host = [0xbb; 32];
        let t = transcript(0, "s1", 9000, &client, &host);
        assert_eq!(&t[..11], b"rd-p0-auth\0");
        assert_eq!(t.len(), 11 + 1 + 2 + 1 + 2 + 7 + 32 + 4 + 32);
        let hp = hex::encode(proof(&TV_SECRET, "host", &t));
        let ap = hex::encode(proof(&TV_SECRET, "android", &t));
        // Golden values (printed once, then pinned; Kotlin AuthTest pins the same).
        assert_eq!(hp, "9869086fdafc487465c1f9a92838c1e81859d9f79942b7209830c5e54a8029c7");
        assert_eq!(ap, "4d01e7857778797c5e4c30f59fcffd20a3f1ae5df07b176d4717293d294b938f");
        assert!(verify(&TV_SECRET, "host", &t, &hp));
        assert!(verify(&TV_SECRET, "android", &t, &ap));
        assert!(!verify(&TV_SECRET, "android", &t, &hp), "role reflection must fail");
        assert!(!verify(&[1u8; 32], "host", &t, &hp), "wrong secret must fail");
        assert!(!verify(&TV_SECRET, "host", &t, "zz"));
        assert!(!verify(&TV_SECRET, "host", &t, &hp[..62]));
        let t2 = transcript(0, "s1", 9001, &client, &host);
        assert!(!verify(&TV_SECRET, "host", &t2, &hp), "port change must fail");
    }

    #[test]
    fn record_layout() {
        let r = bootstrap_record("p0-abc", &TV_SECRET).unwrap();
        assert_eq!(&r[0..4], b"RDB0");
        assert_eq!(r[4], 0);
        assert_eq!(&r[5..8], &[0, 0, 0]);
        assert_eq!(&r[8..14], b"p0-abc");
        assert!(r[14..48].iter().all(|b| *b == 0));
        assert_eq!(&r[48..80], &TV_SECRET);
        assert!(bootstrap_record("", &TV_SECRET).is_err());
        assert!(bootstrap_record(&"x".repeat(41), &TV_SECRET).is_err());
        assert!(bootstrap_record("a\0b", &TV_SECRET).is_err());
        assert!(bootstrap_record(&"x".repeat(40), &TV_SECRET).is_ok());
    }

    #[test]
    fn secret_hex_round_trip() {
        let s = secret_from_hex(&format!("{}\n", hex::encode(TV_SECRET))).unwrap();
        assert_eq!(*s, TV_SECRET);
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert!(secret_from_hex("abcd").is_err());
        assert!(nonce_from_hex("00").is_none());
        assert_eq!(nonce_from_hex(&hex::encode([7u8; 32])), Some([7u8; 32]));
    }
}
