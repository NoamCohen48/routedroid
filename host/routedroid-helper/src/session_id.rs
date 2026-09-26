//! Session identity: a random 64-bit id, and the ownership tag every kernel
//! object of the session carries (TUN `ifalias`, nft table `comment`), so
//! undo and recovery touch only what this session created, never another
//! session's object that happens to have the same name.

use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct SessionId(u64);

impl SessionId {
    /// A fresh id from the kernel's CSPRNG: unique across processes and boots.
    pub fn random() -> Result<Self> {
        let mut bytes = [0u8; 8];
        let n = rustix::rand::getrandom(&mut bytes, rustix::rand::GetRandomFlags::empty()).context("getrandom")?;
        if n != bytes.len() {
            bail!("getrandom returned {n} of {} bytes", bytes.len());
        }
        Ok(Self(u64::from_ne_bytes(bytes)))
    }

    /// The tag written on the session's kernel objects.
    pub fn tag(self) -> String {
        format!("routedroid:{self}")
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl FromStr for SessionId {
    type Err = anyhow::Error;

    /// Exactly the 16 lowercase hex digits `Display` writes, nothing else.
    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 16 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            bail!("session id {s:?} is not 16 lowercase hex digits");
        }
        Ok(Self(u64::from_str_radix(s, 16)?))
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for SessionId {
    type Error = anyhow::Error;

    fn try_from(s: String) -> Result<Self> {
        s.parse()
    }
}

#[cfg(test)]
impl SessionId {
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text_and_json() {
        let id = SessionId(0x00ab_cdef_0123_4567);
        assert_eq!(id.to_string(), "00abcdef01234567");
        assert_eq!("00abcdef01234567".parse::<SessionId>().unwrap(), id);
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"00abcdef01234567\"");
        assert_eq!(id.tag(), "routedroid:00abcdef01234567");
    }

    #[test]
    fn rejects_anything_display_would_not_write() {
        for bad in ["", "abc", "00ABCDEF01234567", "+0abcdef01234567", "00abcdef012345678", "../../etc/passwd"] {
            assert!(bad.parse::<SessionId>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn random_ids_differ() {
        assert_ne!(SessionId::random().unwrap(), SessionId::random().unwrap());
    }
}
