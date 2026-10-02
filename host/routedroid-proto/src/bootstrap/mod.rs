//! The 80-byte bootstrap record streamed to the app's provider (§7.1).
//!
//! ```text
//! "RDB1"[4] | version u8 = 1 | reserved u8 = 0 | device_port u16be
//!           | session[40] NUL-padded | secret[32]
//! ```
//!
//! The record is the only trusted input the app has before AUTH: the session
//! the launch must name and the port it must connect to both come from here.

use zeroize::Zeroizing;

use crate::auth::{Secret, SECRET_LEN};
use crate::messages::valid_session;
use crate::PROTOCOL_VERSION;

pub const RECORD_LEN: usize = 80;
pub const MAGIC: &[u8; 4] = b"RDB1";
pub const SESSION_FIELD_LEN: usize = 40;
pub const PROVIDER_URI: &str = "content://dev.routedroid.bootstrap/record";

const SESSION_AT: usize = 8;
const SECRET_AT: usize = SESSION_AT + SESSION_FIELD_LEN;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error("record must be exactly {RECORD_LEN} bytes, got {0}")]
    Length(usize),
    #[error("bad magic")]
    Magic,
    #[error("unsupported record version {0}")]
    Version(u8),
    #[error("reserved byte is not zero")]
    Reserved,
    #[error("device port 0")]
    Port,
    #[error("session field is not a valid session id")]
    Session,
}

/// A decoded record; the secret zeroizes itself on drop.
#[derive(Debug)]
pub struct Record {
    pub session: String,
    pub device_port: u16,
    pub secret: Secret,
}

/// Build a record. The returned buffer is zeroized on drop because it
/// contains the secret. `None` for an invalid session id or port 0.
pub fn encode(session: &str, device_port: u16, secret: &Secret) -> Option<Zeroizing<[u8; RECORD_LEN]>> {
    if !valid_session(session) || device_port == 0 {
        return None;
    }
    let mut r = Zeroizing::new([0u8; RECORD_LEN]);
    r[..4].copy_from_slice(MAGIC);
    r[4] = PROTOCOL_VERSION;
    r[6..8].copy_from_slice(&device_port.to_be_bytes());
    r[SESSION_AT..SESSION_AT + session.len()].copy_from_slice(session.as_bytes());
    r[SECRET_AT..].copy_from_slice(secret.as_bytes());
    Some(r)
}

/// Parse a record the way the app does; used by tests and the fake app.
pub fn decode(bytes: &[u8]) -> Result<Record, RecordError> {
    if bytes.len() != RECORD_LEN {
        return Err(RecordError::Length(bytes.len()));
    }
    if &bytes[..4] != MAGIC {
        return Err(RecordError::Magic);
    }
    if bytes[4] != PROTOCOL_VERSION {
        return Err(RecordError::Version(bytes[4]));
    }
    if bytes[5] != 0 {
        return Err(RecordError::Reserved);
    }
    let device_port = u16::from_be_bytes([bytes[6], bytes[7]]);
    if device_port == 0 {
        return Err(RecordError::Port);
    }
    let field = &bytes[SESSION_AT..SECRET_AT];
    let end = field.iter().position(|b| *b == 0).unwrap_or(SESSION_FIELD_LEN);
    let session = std::str::from_utf8(&field[..end]).map_err(|_| RecordError::Session)?;
    if !valid_session(session) || field[end..].iter().any(|b| *b != 0) {
        return Err(RecordError::Session);
    }
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&bytes[SECRET_AT..]);
    Ok(Record { session: session.to_string(), device_port, secret: Secret::new(secret) })
}

#[cfg(test)]
mod tests;
