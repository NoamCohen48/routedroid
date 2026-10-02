//! Frame header and limits (§2, §3).
//!
//! ```text
//! u32 body_length   // bytes after the header
//! u8  version       // must be 1
//! u8  message_type
//! u16 flags         // must be 0
//! ```
//!
//! Every limit is checked on the 8-byte header *before* any body buffer is
//! allocated, so a hostile `body_length` of `0xFFFF_FFFF` never allocates.

use crate::PROTOCOL_VERSION;

mod error;
mod message_type;
pub use error::FrameError;
pub use message_type::MessageType;

pub const HEADER_LEN: usize = 8;
/// Control bodies (all JSON messages) are limited to 64 KiB.
pub const MAX_CONTROL_BODY: u32 = 65_536;
/// An IP_PACKET body holds at least a bare IPv4 header; §6 judges the packet itself.
pub const MIN_PACKET_BODY: u32 = 20;
/// Absolute IPv4 total-length limit; the negotiated MTU can only lower it.
pub const MAX_PACKET_BODY: u32 = 65_535;
/// Smallest MTU a host may negotiate (§4.2).
pub const MIN_MTU: u32 = 576;
/// The MTU the host proposes unless told otherwise.
pub const DEFAULT_MTU: u32 = 1400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub message_type: MessageType,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn new(message_type: MessageType, body: Vec<u8>) -> Self {
        Self { message_type, body }
    }

    pub fn empty(message_type: MessageType) -> Self {
        Self::new(message_type, Vec::new())
    }

    pub fn json<T: serde::Serialize>(message_type: MessageType, value: &T) -> Self {
        Self::new(
            message_type,
            serde_json::to_vec(value).expect("control bodies are plain structs"),
        )
    }

    pub fn ip_packet(packet: Vec<u8>) -> Self {
        Self::new(MessageType::IpPacket, packet)
    }

    /// Serialize header + body into a fresh buffer.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len());
        self.encode_into(&mut out);
        out
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let len = u32::try_from(self.body.len()).expect("body length fits in u32");
        out.extend_from_slice(&len.to_be_bytes());
        out.push(PROTOCOL_VERSION);
        out.push(self.message_type as u8);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&self.body);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHeader {
    pub body_length: u32,
    pub version: u8,
    pub message_type: u8,
    pub flags: u16,
}

impl RawHeader {
    pub fn parse(bytes: &[u8; HEADER_LEN]) -> Self {
        Self {
            body_length: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            version: bytes[4],
            message_type: bytes[5],
            flags: u16::from_be_bytes([bytes[6], bytes[7]]),
        }
    }
}

/// Validate a parsed header against the protocol limits. Returns the message
/// type when the header is acceptable. Pure; allocates nothing.
pub fn validate_header(h: &RawHeader, mtu: u32) -> Result<MessageType, FrameError> {
    if h.version != PROTOCOL_VERSION {
        return Err(FrameError::UnsupportedVersion(h.version));
    }
    if h.flags != 0 {
        return Err(FrameError::NonZeroFlags(h.flags));
    }
    let message_type = MessageType::from_u8(h.message_type)
        .ok_or(FrameError::UnknownMessageType(h.message_type))?;
    let len = h.body_length;
    match message_type {
        MessageType::IpPacket => {
            let max = mtu.min(MAX_PACKET_BODY);
            if len < MIN_PACKET_BODY || len > max {
                return Err(FrameError::PacketBodyOutOfRange {
                    body_length: len,
                    mtu: max,
                });
            }
        }
        t if t.is_empty_body() => {
            if len != 0 {
                return Err(FrameError::UnexpectedBody {
                    message_type: t,
                    body_length: len,
                });
            }
        }
        t => {
            if len == 0 {
                return Err(FrameError::EmptyBody(t));
            }
            if len > MAX_CONTROL_BODY {
                return Err(FrameError::ControlBodyTooLarge {
                    message_type: t,
                    body_length: len,
                });
            }
        }
    }
    Ok(message_type)
}

/// Decode one frame from a byte slice. Returns the frame and the number of
/// bytes consumed. Header validation happens before the body is touched.
pub fn decode(bytes: &[u8], mtu: u32) -> Result<(Frame, usize), FrameError> {
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::Truncated {
            clean: bytes.is_empty(),
        });
    }
    let mut hdr = [0u8; HEADER_LEN];
    hdr.copy_from_slice(&bytes[..HEADER_LEN]);
    let raw = RawHeader::parse(&hdr);
    let message_type = validate_header(&raw, mtu)?;
    let body_len = raw.body_length as usize;
    let end = HEADER_LEN + body_len;
    if bytes.len() < end {
        return Err(FrameError::Truncated { clean: false });
    }
    Ok((
        Frame::new(message_type, bytes[HEADER_LEN..end].to_vec()),
        end,
    ))
}

#[cfg(feature = "tokio")]
mod reader;
#[cfg(feature = "tokio")]
pub use reader::read_frame;

#[cfg(test)]
mod tests;
