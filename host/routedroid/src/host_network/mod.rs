//! The host's network footprint for a session (TUN, LAN alias, proxy ARP),
//! obtained from the privileged helper process (architecture §5.3). The
//! helper owns every network mutation; this module only asks and relays.

mod client;

pub use client::{HostNetwork, DEFAULT_SOCKET};
