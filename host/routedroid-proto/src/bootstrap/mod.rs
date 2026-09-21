//! The 80-byte bootstrap record streamed to the app's provider (§7.1).
//!
//! ```text
//! "RDB1"[4] | version u8 = 1 | reserved[3] = 0 | session[40] NUL-padded | secret[32]
//! ```

use zeroize::Zeroizing;

use crate::auth::{Secret, SECRET_LEN};
use crate::messages::valid_session;
use crate::PROTOCOL_VERSION;

pub const RECORD_LEN: usize = 80;
pub const MAGIC: &[u8; 4] = b"RDB1";
pub const SESSION_FIELD_LEN: usize = 40;
pub const PROVIDER_URI: &str = "content://dev.routedroid.bootstrap/record";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error("record must be exactly {RECORD_LEN} bytes, got {0}")]
    Length(usize),
    #[error("bad magic")]
    Magic,
    #[error("unsupported record version {0}")]
    Version(u8),
    #[error("session field is not a valid session id")]
    Session,
}

/// Build a record. The returned buffer is zeroized on drop because it
/// contains the secret.
pub fn encode(session: &str, secret: &Secret) -> Option<Zeroizing<[u8; RECORD_LEN]>> {
    if !valid_session(session) {
        return None;
    }
    let mut r = Zeroizing::new([0u8; RECORD_LEN]);
    r[..4].copy_from_slice(MAGIC);
    r[4] = PROTOCOL_VERSION;
    r[8..8 + session.len()].copy_from_slice(session.as_bytes());
    r[48..].copy_from_slice(secret.as_bytes());
    Some(r)
}

/// Parse a record the way the app does; used by tests and the fake app.
pub fn decode(bytes: &[u8]) -> Result<(String, Secret), RecordError> {
    if bytes.len() != RECORD_LEN {
        return Err(RecordError::Length(bytes.len()));
    }
    if &bytes[..4] != MAGIC {
        return Err(RecordError::Magic);
    }
    if bytes[4] != PROTOCOL_VERSION {
        return Err(RecordError::Version(bytes[4]));
    }
    let field = &bytes[8..8 + SESSION_FIELD_LEN];
    let end = field.iter().position(|b| *b == 0).unwrap_or(SESSION_FIELD_LEN);
    let session = std::str::from_utf8(&field[..end]).map_err(|_| RecordError::Session)?;
    if !valid_session(session) || field[end..].iter().any(|b| *b != 0) {
        return Err(RecordError::Session);
    }
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&bytes[48..]);
    Ok((session.to_string(), Secret::new(secret)))
}

#[cfg(test)]
mod tests;
