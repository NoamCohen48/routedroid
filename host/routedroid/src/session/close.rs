use std::fmt;

use routedroid_proto::messages::ErrorBody;

/// Why the machine wants the connection closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Close {
    /// Peer sent STOP.
    PeerStop,
    /// Peer sent VPN_ERROR.
    VpnError(ErrorBody),
    /// We detected a violation or a refusal; send this ERROR, then close.
    Refuse(ErrorBody),
}

impl fmt::Display for Close {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerStop => write!(f, "peer sent STOP"),
            Self::VpnError(e) => write!(f, "peer sent VPN_ERROR {}: {}", e.code, e.message),
            Self::Refuse(e) => write!(f, "{}: {}", e.code, e.message),
        }
    }
}

/// How a driven session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// We were asked to stop; STOP was sent to the peer.
    LocalStop,
    /// Peer closed the TCP stream at a frame boundary.
    PeerClosed,
    PeerStop,
    VpnError(ErrorBody),
    /// We refused the peer (protocol violation, auth failure, mismatch).
    Refused(ErrorBody),
    /// No frame at all for the keepalive deadline.
    KeepaliveTimeout,
    /// TCP transport failure.
    Transport(String),
    /// The helper's packet channel went away.
    HelperClosed,
}

impl fmt::Display for SessionEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalStop => write!(f, "local stop"),
            Self::PeerClosed => write!(f, "peer closed connection"),
            Self::PeerStop => write!(f, "peer sent STOP"),
            Self::VpnError(e) => write!(f, "peer VPN_ERROR {}: {}", e.code, e.message),
            Self::Refused(e) => write!(f, "refused peer: {}: {}", e.code, e.message),
            Self::KeepaliveTimeout => write!(f, "keepalive timeout"),
            Self::Transport(s) => write!(f, "transport error: {s}"),
            Self::HelperClosed => write!(f, "helper packet channel closed"),
        }
    }
}
