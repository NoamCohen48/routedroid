//! Error taxonomy: every failure the daemon reports belongs to one kind, so
//! a client can branch on it (the CLI turns each into an exit code).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Bad arguments or configuration.
    Usage,
    /// ADB missing, device missing, unauthorized, or a command failed.
    Adb,
    /// The USB-only rule or another transport rule was violated.
    Transport,
    /// The phone spoke the protocol wrongly.
    Protocol,
    /// Mutual authentication failed.
    Auth,
    /// The phone refused or failed to bring the VPN up.
    Vpn,
    /// The privileged helper refused or failed.
    Helper,
    /// Something did not happen in time: the app never dialled in, or a
    /// teardown is still running.
    Timeout,
    /// Anything unexpected.
    Internal,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Self::Usage,
        Self::Adb,
        Self::Transport,
        Self::Protocol,
        Self::Auth,
        Self::Vpn,
        Self::Helper,
        Self::Timeout,
        Self::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Adb => "adb",
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Auth => "auth",
            Self::Vpn => "vpn",
            Self::Helper => "helper",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
