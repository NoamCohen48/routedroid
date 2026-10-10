//! The host's network footprint for a connection (TUN, the phone's /32
//! route, proxy ARP, the session firewall, the phone's DHCP lease),
//! obtained from the privileged helper process (architecture §5.3). The
//! helper owns every network mutation; this module only asks and relays.
//! No address is ever added to the LAN interface.

mod client;
mod doctor;
mod gateway;
mod helper;
mod interfaces;
mod relay;

pub use client::HostNetwork;
pub use doctor::doctor;
pub use gateway::default_gateway;
pub use helper::connect;
pub use interfaces::interfaces;
pub use relay::HelperEvent;
pub use routedroid_helper_ipc::DEFAULT_SOCKET;
