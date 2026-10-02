//! A device connection's life on the wire: its state, how it ended, and
//! its traffic.

use serde::{Deserialize, Serialize};

use crate::Kind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    /// Helper session and reverse mapping being set up (and, without a
    /// static address, the DHCP lease being acquired).
    Starting,
    /// App launched; waiting for it to dial in (consent dialog may be up).
    WaitingForApp,
    /// App connected; the HELLO/AUTH/CONFIGURE handshake is in progress.
    Handshaking,
    Active,
    Stopping,
    Ended {
        outcome: Outcome,
    },
}

/// How a connection ended: on purpose, or because something failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    Clean { reason: EndReason },
    Failed { kind: Kind, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// `stop` (or daemon shutdown) before the connection was active.
    StoppedEarly,
    /// `stop` (or daemon shutdown) while it was active.
    Stopped,
    /// Stopped on the phone: the app's Stop button or its notification.
    PhoneStopped,
    /// The phone closed the connection after an active session.
    PhoneClosed,
}

/// Counters of one connection since it became active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traffic {
    pub packets_to_phone: u64,
    pub packets_from_phone: u64,
    pub bytes_to_phone: u64,
    pub bytes_from_phone: u64,
    /// Phone packets dropped because they failed the protocol's IPv4 checks.
    pub dropped_malformed: u64,
    /// Phone packets dropped because the host's queue was full.
    pub dropped_congested: u64,
}

impl Outcome {
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Clean { .. })
    }

    pub fn failed(kind: Kind, message: impl Into<String>) -> Self {
        Self::Failed {
            kind,
            message: message.into(),
        }
    }
}
