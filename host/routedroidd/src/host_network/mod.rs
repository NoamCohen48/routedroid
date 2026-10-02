//! The host's network footprint for a connection (TUN, the phone's /32
//! route, proxy ARP, the session firewall), obtained from the privileged
//! helper process (architecture §5.3). The helper owns every network
//! mutation; this module only asks and relays. No address is ever added to
//! the LAN interface.

mod client;
mod gateway;
mod helper;
mod interfaces;

pub use client::HostNetwork;
pub use gateway::default_gateway;
pub use interfaces::interfaces;
pub use routedroid_helper_ipc::DEFAULT_SOCKET;
