//! Error taxonomy: every failure the daemon reports belongs to one kind, and
//! each kind has a stable CLI exit code so scripts can branch on it.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Anything unexpected.
    Internal,
    /// A kind this client does not know (newer daemon).
    #[serde(other)]
    Unknown,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Adb => "adb",
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Auth => "auth",
            Self::Vpn => "vpn",
            Self::Helper => "helper",
            Self::Internal => "internal",
            Self::Unknown => "unknown",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::Usage => 2,
            Self::Adb => 10,
            Self::Transport => 11,
            Self::Protocol => 12,
            Self::Auth => 13,
            Self::Vpn => 14,
            Self::Helper => 15,
            Self::Internal | Self::Unknown => 70,
        }
    }
}

#[derive(Debug)]
pub struct Fault {
    kind: Kind,
    source: anyhow::Error,
}

impl Fault {
    pub fn new(kind: Kind, source: impl Into<anyhow::Error>) -> Self {
        Self { kind, source: source.into() }
    }

    pub fn msg(kind: Kind, message: impl fmt::Display) -> Self {
        Self { kind, source: anyhow::anyhow!("{message}") }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.source)
    }
}

impl std::error::Error for Fault {}

pub type Result<T> = std::result::Result<T, Fault>;

/// `.fault(Kind::Adb)` on any `anyhow`/`std` result.
pub trait FaultExt<T> {
    fn fault(self, kind: Kind) -> Result<T>;
}

impl<T, E: Into<anyhow::Error>> FaultExt<T> for std::result::Result<T, E> {
    fn fault(self, kind: Kind) -> Result<T> {
        self.map_err(|e| Fault::new(kind, e))
    }
}
