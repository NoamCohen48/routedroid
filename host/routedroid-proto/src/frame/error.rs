//! Header rejection reasons (§2, §8).

use super::{MessageType, MAX_CONTROL_BODY, MIN_PACKET_BODY};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[error("nonzero flags 0x{0:04x}")]
    NonZeroFlags(u16),
    #[error("unknown message type 0x{0:02x}")]
    UnknownMessageType(u8),
    #[error("{message_type} body of {body_length} bytes exceeds control limit {MAX_CONTROL_BODY}")]
    ControlBodyTooLarge {
        message_type: MessageType,
        body_length: u32,
    },
    #[error("{message_type} must be empty, got {body_length} bytes")]
    UnexpectedBody {
        message_type: MessageType,
        body_length: u32,
    },
    #[error("{0} must carry a body")]
    EmptyBody(MessageType),
    #[error("IP_PACKET body of {body_length} bytes outside [{MIN_PACKET_BODY}, {mtu}]")]
    PacketBodyOutOfRange { body_length: u32, mtu: u32 },
    /// Stream ended in the middle of a frame (or cleanly, `clean == true`,
    /// exactly at a frame boundary).
    #[error("{}", if *.clean { "peer closed the stream" } else { "stream ended mid-frame" })]
    Truncated { clean: bool },
    #[error("i/o error: {0}")]
    Io(String),
}

impl FrameError {
    /// The rejection code used by `protocol/fixtures/frames.json`.
    pub fn fixture_code(&self) -> &'static str {
        match self {
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::NonZeroFlags(_) => "nonzero_flags",
            Self::UnknownMessageType(_) => "unknown_type",
            Self::ControlBodyTooLarge { .. } => "control_body_too_large",
            Self::UnexpectedBody { .. } => "unexpected_body",
            Self::EmptyBody(_) => "empty_body",
            Self::PacketBodyOutOfRange { .. } => "packet_body_out_of_range",
            Self::Truncated { .. } => "truncated",
            Self::Io(_) => "io",
        }
    }
}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
