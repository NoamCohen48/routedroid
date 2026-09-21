//! Phase 0 wire framing (protocol/phase0-draft.md).
//!
//! ```text
//! u32 body_length   // excludes header; 0 allowed only for PING/PONG/STOP
//! u8  version       // must be 0
//! u8  message_type
//! u16 flags         // must be 0
//! ```
//!
//! Every limit is checked on the 8-byte header *before* any body buffer is
//! allocated, so a hostile `body_length` of `0xFFFF_FFFF` never allocates.

use std::fmt;

use tokio::io::{AsyncRead, AsyncReadExt};

pub const HEADER_LEN: usize = 8;
pub const PROTOCOL_VERSION: u8 = 0;
/// Control bodies (all JSON messages) are limited to 64 KiB.
pub const MAX_CONTROL_BODY: u32 = 65536;
/// IP_PACKET bodies must be strictly larger than an IPv4 header.
pub const MIN_PACKET_BODY: u32 = 21;
/// Default MTU when nothing else was negotiated (draft: 1400).
pub const DEFAULT_MTU: u32 = 1400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    Hello = 0x01,
    HelloAck = 0x02,
    ConfigureVpn = 0x03,
    VpnReady = 0x04,
    VpnError = 0x05,
    Auth = 0x06,
    IpPacket = 0x10,
    Ping = 0x20,
    Pong = 0x21,
    Stop = 0x30,
    Error = 0x7F,
}

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0x01 => Self::Hello,
            0x02 => Self::HelloAck,
            0x03 => Self::ConfigureVpn,
            0x04 => Self::VpnReady,
            0x05 => Self::VpnError,
            0x06 => Self::Auth,
            0x10 => Self::IpPacket,
            0x20 => Self::Ping,
            0x21 => Self::Pong,
            0x30 => Self::Stop,
            0x7F => Self::Error,
            _ => return None,
        })
    }

    /// Messages whose body must be empty.
    pub fn is_empty_body(self) -> bool {
        matches!(self, Self::Ping | Self::Pong | Self::Stop)
    }
}

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
        Self::new(message_type, serde_json::to_vec(value).expect("control bodies are plain structs"))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    UnsupportedVersion(u8),
    NonZeroFlags(u16),
    UnknownMessageType(u8),
    /// Control body larger than [`MAX_CONTROL_BODY`].
    ControlBodyTooLarge {
        message_type: MessageType,
        body_length: u32,
    },
    /// PING/PONG/STOP carrying a body.
    UnexpectedBody {
        message_type: MessageType,
        body_length: u32,
    },
    /// Zero-length body on a message that requires one (JSON or IP_PACKET).
    EmptyBody(MessageType),
    /// IP_PACKET body outside `(20, mtu]`.
    PacketBodyOutOfRange {
        body_length: u32,
        mtu: u32,
    },
    /// Stream ended in the middle of a frame (or cleanly, `clean == true`,
    /// exactly at a frame boundary).
    Truncated {
        clean: bool,
    },
    Io(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "unsupported protocol version {v}"),
            Self::NonZeroFlags(x) => write!(f, "nonzero flags 0x{x:04x}"),
            Self::UnknownMessageType(t) => write!(f, "unknown message type 0x{t:02x}"),
            Self::ControlBodyTooLarge { message_type, body_length } => {
                write!(f, "{message_type:?} body of {body_length} bytes exceeds control limit {MAX_CONTROL_BODY}")
            }
            Self::UnexpectedBody { message_type, body_length } => {
                write!(f, "{message_type:?} must be empty, got {body_length} bytes")
            }
            Self::EmptyBody(t) => write!(f, "{t:?} must carry a body"),
            Self::PacketBodyOutOfRange { body_length, mtu } => {
                write!(f, "IP_PACKET body of {body_length} bytes outside ({}, {mtu}]", MIN_PACKET_BODY - 1)
            }
            Self::Truncated { clean: true } => write!(f, "peer closed the stream"),
            Self::Truncated { clean: false } => write!(f, "stream ended mid-frame"),
            Self::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
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
    let message_type = MessageType::from_u8(h.message_type).ok_or(FrameError::UnknownMessageType(h.message_type))?;
    let len = h.body_length;
    match message_type {
        MessageType::IpPacket => {
            if len < MIN_PACKET_BODY || len > mtu {
                return Err(FrameError::PacketBodyOutOfRange { body_length: len, mtu });
            }
        }
        t if t.is_empty_body() => {
            if len != 0 {
                return Err(FrameError::UnexpectedBody { message_type: t, body_length: len });
            }
        }
        t => {
            if len == 0 {
                return Err(FrameError::EmptyBody(t));
            }
            if len > MAX_CONTROL_BODY {
                return Err(FrameError::ControlBodyTooLarge { message_type: t, body_length: len });
            }
        }
    }
    Ok(message_type)
}

/// Decode one frame from a byte slice. Returns the frame and the number of
/// bytes consumed. Header validation happens before the body is touched.
#[cfg_attr(not(test), allow(dead_code))]
pub fn decode(bytes: &[u8], mtu: u32) -> Result<(Frame, usize), FrameError> {
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::Truncated { clean: bytes.is_empty() });
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
    Ok((Frame::new(message_type, bytes[HEADER_LEN..end].to_vec()), end))
}

/// Read exactly one frame from an async stream. The body buffer is allocated
/// only after the header passed [`validate_header`].
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R, mtu: u32) -> Result<Frame, FrameError> {
    let mut hdr = [0u8; HEADER_LEN];
    let mut filled = 0usize;
    while filled < HEADER_LEN {
        let n = reader.read(&mut hdr[filled..]).await?;
        if n == 0 {
            return Err(FrameError::Truncated { clean: filled == 0 });
        }
        filled += n;
    }
    let raw = RawHeader::parse(&hdr);
    let message_type = validate_header(&raw, mtu)?;
    let mut body = vec![0u8; raw.body_length as usize];
    if !body.is_empty() {
        reader.read_exact(&mut body).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => FrameError::Truncated { clean: false },
            _ => FrameError::Io(e.to_string()),
        })?;
    }
    Ok(Frame::new(message_type, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HELLO golden vector: the exact bytes both implementations must produce.
    const HELLO_BODY: &[u8] = br#"{"protocol":0,"session":"abc","device_port":9000}"#;
    const HELLO_WIRE: &[u8] = &[
        0x00, 0x00, 0x00, 0x31, // body_length = 49
        0x00, // version 0
        0x01, // HELLO
        0x00, 0x00, // flags
        b'{', b'"', b'p', b'r', b'o', b't', b'o', b'c', b'o', b'l', b'"', b':', b'0', b',', b'"', b's', b'e', b's',
        b's', b'i', b'o', b'n', b'"', b':', b'"', b'a', b'b', b'c', b'"', b',', b'"', b'd', b'e', b'v', b'i', b'c',
        b'e', b'_', b'p', b'o', b'r', b't', b'"', b':', b'9', b'0', b'0', b'0', b'}',
    ];

    /// IP_PACKET golden vector: 28-byte IPv4/ICMP echo request 10.0.0.2 -> 10.0.0.1.
    const PACKET_BODY: &[u8] = &[
        0x45, 0x00, 0x00, 0x1c, 0x00, 0x01, 0x00, 0x00, 0x40, 0x01, 0x66, 0xde, 0x0a, 0x00, 0x00, 0x02, 0x0a, 0x00,
        0x00, 0x01, 0x08, 0x00, 0xf7, 0xff, 0x00, 0x00, 0x00, 0x00,
    ];
    const PACKET_WIRE: &[u8] = &[
        0x00, 0x00, 0x00, 0x1c, // body_length = 28
        0x00, // version
        0x10, // IP_PACKET
        0x00, 0x00, // flags
        0x45, 0x00, 0x00, 0x1c, 0x00, 0x01, 0x00, 0x00, 0x40, 0x01, 0x66, 0xde, 0x0a, 0x00, 0x00, 0x02, 0x0a, 0x00,
        0x00, 0x01, 0x08, 0x00, 0xf7, 0xff, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn hello_golden_encode() {
        let f = Frame::new(MessageType::Hello, HELLO_BODY.to_vec());
        assert_eq!(f.encode(), HELLO_WIRE);
        assert_eq!(HELLO_WIRE.len(), HEADER_LEN + 49);
    }

    /// Same vector as the Android probe's test: session "s" -> 47-byte body.
    #[test]
    fn hello_golden_matches_android_probe() {
        let body = br#"{"protocol":0,"session":"s","device_port":9000}"#;
        assert_eq!(body.len(), 47);
        let f = Frame::new(MessageType::Hello, body.to_vec());
        let wire = f.encode();
        assert_eq!(&wire[..HEADER_LEN], &[0x00, 0x00, 0x00, 0x2F, 0x00, 0x01, 0x00, 0x00]);
        assert_eq!(&wire[HEADER_LEN..], body);
        assert_eq!(decode(&wire, DEFAULT_MTU).unwrap().0, f);
    }

    #[test]
    fn hello_golden_decode() {
        let (f, used) = decode(HELLO_WIRE, DEFAULT_MTU).unwrap();
        assert_eq!(used, HELLO_WIRE.len());
        assert_eq!(f.message_type, MessageType::Hello);
        assert_eq!(f.body, HELLO_BODY);
    }

    #[test]
    fn ip_packet_golden_roundtrip() {
        let f = Frame::ip_packet(PACKET_BODY.to_vec());
        assert_eq!(f.encode(), PACKET_WIRE);
        let (d, used) = decode(PACKET_WIRE, DEFAULT_MTU).unwrap();
        assert_eq!(used, PACKET_WIRE.len());
        assert_eq!(d, f);
    }

    #[test]
    fn empty_control_frames_encode_as_header_only() {
        assert_eq!(Frame::empty(MessageType::Ping).encode(), [0, 0, 0, 0, 0, 0x20, 0, 0]);
        assert_eq!(Frame::empty(MessageType::Pong).encode(), [0, 0, 0, 0, 0, 0x21, 0, 0]);
        assert_eq!(Frame::empty(MessageType::Stop).encode(), [0, 0, 0, 0, 0, 0x30, 0, 0]);
        let (f, _) = decode(&[0, 0, 0, 0, 0, 0x30, 0, 0], DEFAULT_MTU).unwrap();
        assert_eq!(f, Frame::empty(MessageType::Stop));
    }

    fn with_header(mut wire: Vec<u8>, patch: impl FnOnce(&mut [u8])) -> Vec<u8> {
        patch(&mut wire[..HEADER_LEN]);
        wire
    }

    #[test]
    fn rejects_nonzero_flags() {
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[7] = 0x01);
        assert_eq!(decode(&wire, DEFAULT_MTU).unwrap_err(), FrameError::NonZeroFlags(1));
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[6] = 0x80);
        assert_eq!(decode(&wire, DEFAULT_MTU).unwrap_err(), FrameError::NonZeroFlags(0x8000));
    }

    #[test]
    fn rejects_wrong_version() {
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[4] = 1);
        assert_eq!(decode(&wire, DEFAULT_MTU).unwrap_err(), FrameError::UnsupportedVersion(1));
    }

    #[test]
    fn rejects_unknown_type() {
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[5] = 0x11);
        assert_eq!(decode(&wire, DEFAULT_MTU).unwrap_err(), FrameError::UnknownMessageType(0x11));
    }

    #[test]
    fn rejects_control_body_over_limit_without_allocating() {
        // 0xFFFF_FFFF body on a HELLO: must fail on the header alone.
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[..4].copy_from_slice(&[0xff; 4]));
        assert_eq!(
            decode(&wire, DEFAULT_MTU).unwrap_err(),
            FrameError::ControlBodyTooLarge { message_type: MessageType::Hello, body_length: u32::MAX }
        );
        // Exactly the limit is fine at header level; one over is not.
        let ok = RawHeader { body_length: MAX_CONTROL_BODY, version: 0, message_type: 0x01, flags: 0 };
        assert_eq!(validate_header(&ok, DEFAULT_MTU), Ok(MessageType::Hello));
        let bad = RawHeader { body_length: MAX_CONTROL_BODY + 1, ..ok };
        assert!(matches!(validate_header(&bad, DEFAULT_MTU), Err(FrameError::ControlBodyTooLarge { .. })));
    }

    #[test]
    fn rejects_packet_body_out_of_range() {
        for len in [0u32, 1, 20, DEFAULT_MTU + 1, u32::MAX] {
            let h = RawHeader { body_length: len, version: 0, message_type: 0x10, flags: 0 };
            assert_eq!(
                validate_header(&h, DEFAULT_MTU),
                Err(FrameError::PacketBodyOutOfRange { body_length: len, mtu: DEFAULT_MTU }),
                "len {len}"
            );
        }
        for len in [21u32, 28, DEFAULT_MTU] {
            let h = RawHeader { body_length: len, version: 0, message_type: 0x10, flags: 0 };
            assert_eq!(validate_header(&h, DEFAULT_MTU), Ok(MessageType::IpPacket));
        }
    }

    #[test]
    fn rejects_zero_length_ip_packet() {
        let wire = [0, 0, 0, 0, 0, 0x10, 0, 0];
        assert_eq!(
            decode(&wire, DEFAULT_MTU).unwrap_err(),
            FrameError::PacketBodyOutOfRange { body_length: 0, mtu: DEFAULT_MTU }
        );
    }

    #[test]
    fn rejects_empty_json_body_and_nonempty_ping() {
        assert_eq!(
            decode(&[0, 0, 0, 0, 0, 0x01, 0, 0], DEFAULT_MTU).unwrap_err(),
            FrameError::EmptyBody(MessageType::Hello)
        );
        assert_eq!(
            decode(&[0, 0, 0, 1, 0, 0x20, 0, 0, 0xAA], DEFAULT_MTU).unwrap_err(),
            FrameError::UnexpectedBody { message_type: MessageType::Ping, body_length: 1 }
        );
    }

    #[test]
    fn truncated_frames_are_reported() {
        assert_eq!(decode(&[], DEFAULT_MTU).unwrap_err(), FrameError::Truncated { clean: true });
        assert_eq!(decode(&HELLO_WIRE[..5], DEFAULT_MTU).unwrap_err(), FrameError::Truncated { clean: false });
        assert_eq!(
            decode(&HELLO_WIRE[..HELLO_WIRE.len() - 1], DEFAULT_MTU).unwrap_err(),
            FrameError::Truncated { clean: false }
        );
    }

    #[tokio::test]
    async fn async_reader_never_allocates_for_hostile_length() {
        // A stream of just the hostile header: the reader must error out before
        // attempting to read (or allocate) 4 GiB.
        let wire = with_header(HELLO_WIRE.to_vec(), |h| h[..4].copy_from_slice(&[0xff; 4]));
        let mut cursor = std::io::Cursor::new(wire);
        let err = read_frame(&mut cursor, DEFAULT_MTU).await.unwrap_err();
        assert!(matches!(err, FrameError::ControlBodyTooLarge { .. }));
        // Reader stopped right after the header.
        assert_eq!(cursor.position() as usize, HEADER_LEN);
    }

    #[tokio::test]
    async fn async_reader_roundtrip_and_truncation() {
        let mut wire = HELLO_WIRE.to_vec();
        wire.extend_from_slice(PACKET_WIRE);
        let mut cursor = std::io::Cursor::new(wire);
        let a = read_frame(&mut cursor, DEFAULT_MTU).await.unwrap();
        assert_eq!(a.message_type, MessageType::Hello);
        let b = read_frame(&mut cursor, DEFAULT_MTU).await.unwrap();
        assert_eq!(b.body, PACKET_BODY);
        assert_eq!(read_frame(&mut cursor, DEFAULT_MTU).await.unwrap_err(), FrameError::Truncated { clean: true });
        let mut cut = std::io::Cursor::new(PACKET_WIRE[..20].to_vec());
        assert_eq!(read_frame(&mut cut, DEFAULT_MTU).await.unwrap_err(), FrameError::Truncated { clean: false });
    }
}
