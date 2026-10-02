use std::net::Ipv4Addr;

use routedroid_proto::messages::{ConfigureVpn, Prefix};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub mtu: u32,
    pub addresses: Vec<Prefix>,
    pub routes: Vec<Prefix>,
    pub dns: Vec<Ipv4Addr>,
    pub session_name: String,
    /// The HELLO `session` and `device_port` must match what we launched.
    pub expected_session: String,
    pub expected_device_port: u16,
}

impl SessionConfig {
    pub fn configure_vpn(&self) -> ConfigureVpn {
        ConfigureVpn {
            mtu: self.mtu,
            addresses: self.addresses.clone(),
            routes: self.routes.clone(),
            dns: self.dns.clone(),
            session_name: self.session_name.clone(),
        }
    }
}
