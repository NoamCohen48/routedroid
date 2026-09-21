//! Client side of the privileged helper (architecture §5.3). The helper owns
//! the TUN and every network mutation; this module only asks and relays.

mod client;

pub use client::{HelperSession, DEFAULT_SOCKET};
