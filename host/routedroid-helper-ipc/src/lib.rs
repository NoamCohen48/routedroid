//! IPC between routedroidd (the controller) and the privileged helper
//! (architecture §5.3). Both binaries build on this crate, so the wire is
//! defined once: one SOCK_SEQPACKET connection per phone, a mandatory
//! `Hello` version exchange, then typed control messages and raw IPv4
//! packets (`Datagram`).

mod datagram;
mod ifname;
mod message;
mod seqpacket;

pub use datagram::{Datagram, DecodeError, MAX_DATAGRAM, MAX_PACKET};
pub use ifname::{IfName, IfNameError};
pub use message::{ErrorCode, Reply, Request};
pub use seqpacket::{Activated, Activation, Listener, SeqPacket};

/// Bumped on any incompatible change to [`Request`], [`Reply`] or the
/// datagram layout. The helper refuses a controller that says otherwise.
pub const VERSION: u32 = 1;

/// Every TUN the helper creates is named with this prefix; it refuses others.
pub const TUN_PREFIX: &str = "phone";

/// The TUN MTUs a `Start` may ask for; the helper refuses others.
pub const MTU_RANGE: std::ops::RangeInclusive<u32> = 576..=9000;

/// Where the systemd socket unit listens.
pub const DEFAULT_SOCKET: &str = "/run/routedroid/helper.sock";
