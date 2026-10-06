//! The words every client uses for the wire's states, so the CLI and the TUI
//! cannot describe the same connection two ways.

use std::fmt;

use crate::{ConnectionState, EndReason, Outcome};

impl fmt::Display for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::StoppedEarly => "stopped before the connection was active",
            Self::Stopped => "stopped",
            Self::PhoneStopped => "stopped on the phone",
            Self::PhoneClosed => "the phone closed the connection",
        })
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clean { reason } => write!(f, "{reason}"),
            Self::Failed { kind, message } => write!(f, "failed ({kind}): {message}"),
        }
    }
}

/// The short word, for tables; `{:#}` is the sentence an attached `start`
/// prints, which tells the user what to do.
impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting => f.write_str("starting"),
            Self::WaitingForApp if f.alternate() => {
                f.write_str("waiting for the app to connect (answer the VPN dialog on the phone)")
            }
            Self::WaitingForApp => f.write_str("waiting for app"),
            Self::Handshaking => f.write_str("handshaking"),
            Self::Active => f.write_str("active"),
            Self::Reconnecting { wait_secs } if f.alternate() => write!(
                f,
                "the phone went away (unplugged?): its address is held for up to {wait_secs} s while it comes back"
            ),
            Self::Reconnecting { .. } => f.write_str("reconnecting"),
            Self::Stopping => f.write_str("stopping"),
            Self::Ended { outcome } => write!(f, "ended: {outcome}"),
        }
    }
}
