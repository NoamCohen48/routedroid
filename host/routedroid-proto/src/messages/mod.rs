//! JSON control bodies (§4) and their field rules.
//!
//! Serde keeps the field order shown in the spec so encodings are byte-stable
//! against the fixtures; unknown fields are ignored on input (§9). `validate`
//! applies the rules a parser cannot express: ranges, lengths, character sets.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::frame::{MAX_PACKET_BODY, MIN_MTU};
use crate::PROTOCOL_VERSION;

pub const MAX_SESSION_LEN: usize = 40;
pub const MAX_APP_LEN: usize = 64;
pub const MAX_SESSION_NAME_LEN: usize = 64;
pub const MAX_ERROR_MESSAGE_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BodyError {
    #[error("invalid JSON body: {0}")]
    Json(String),
    #[error("field `{field}`: {reason}")]
    Field { field: &'static str, reason: String },
}

impl From<serde_json::Error> for BodyError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

fn field(field: &'static str, reason: impl Into<String>) -> BodyError {
    BodyError::Field {
        field,
        reason: reason.into(),
    }
}

/// §4.1: 1–40 characters from `A-Z a-z 0-9 . _ -`.
pub fn valid_session(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SESSION_LEN
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Parse a control body and apply the spec's field rules.
pub fn parse<T: Body>(body: &[u8]) -> Result<T, BodyError> {
    let v: T = serde_json::from_slice(body)?;
    v.validate()?;
    Ok(v)
}

pub trait Body: Serialize + for<'de> Deserialize<'de> {
    fn validate(&self) -> Result<(), BodyError>;
}

mod error;
mod handshake;
mod hex32;
mod vpn;
pub use error::{ErrorBody, ErrorCode};
pub use handshake::{Auth, Hello, HelloAck};
pub use vpn::{is_unicast_host, ConfigureVpn, Prefix, VpnReady};

#[cfg(test)]
mod tests;
