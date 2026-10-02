//! CONFIGURE_VPN and VPN_READY (§4.4–4.5).

use super::*;

/// `{"address":"a.b.c.d","prefix":n}`. Serde reads and writes the address as
/// a dotted quad and refuses anything else (leading zeros, non-ASCII digits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefix {
    pub address: Ipv4Addr,
    pub prefix: u8,
}

impl Prefix {
    pub fn new(address: Ipv4Addr, prefix: u8) -> Self {
        Self { address, prefix }
    }

    /// No address bits set beyond the prefix (`10.0.0.0/8`, not `10.0.0.1/8`).
    pub fn is_canonical(&self) -> bool {
        self.prefix <= 32 && u32::from(self.address) & !mask(self.prefix) == 0
    }
}

fn mask(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

/// An address a host may own: not unspecified, loopback, multicast, reserved
/// (240/4) or limited broadcast.
pub fn is_unicast_host(a: Ipv4Addr) -> bool {
    !(a.octets()[0] == 0 || a.is_loopback() || a.is_multicast() || a.octets()[0] >= 240)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigureVpn {
    pub mtu: u32,
    pub addresses: Vec<Prefix>,
    pub routes: Vec<Prefix>,
    pub dns: Vec<Ipv4Addr>,
    pub session_name: String,
}

impl Body for ConfigureVpn {
    fn validate(&self) -> Result<(), BodyError> {
        if self.mtu < MIN_MTU || self.mtu > MAX_PACKET_BODY {
            return Err(field("mtu", format!("must be {MIN_MTU}-{MAX_PACKET_BODY}")));
        }
        let [address] = self.addresses.as_slice() else {
            return Err(field("addresses", "exactly one address in version 1"));
        };
        if address.prefix != 32 || !is_unicast_host(address.address) {
            return Err(field("addresses", "a unicast host address with prefix 32"));
        }
        if self.routes.is_empty() {
            return Err(field("routes", "at least one route"));
        }
        if !self.routes.iter().all(Prefix::is_canonical) {
            return Err(field("routes", "prefix 0-32 with no address bits past it"));
        }
        if !self.dns.iter().all(|d| is_unicast_host(*d)) {
            return Err(field("dns", "unicast addresses"));
        }
        if self.session_name.chars().count() > MAX_SESSION_NAME_LEN {
            return Err(field("session_name", "at most 64 characters"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VpnReady {
    pub addresses: Vec<String>,
    pub mtu: u32,
}

impl Body for VpnReady {
    fn validate(&self) -> Result<(), BodyError> {
        for a in &self.addresses {
            let (ip, prefix) = a.split_once('/').ok_or_else(|| field("addresses", "expected ip/prefix"))?;
            check_ipv4("addresses", ip)?;
            if !prefix.bytes().all(|b| b.is_ascii_digit()) || prefix.parse::<u8>().map_or(true, |p| p > 32) {
                return Err(field("addresses", "prefix must be 0-32"));
            }
        }
        if self.mtu < MIN_MTU || self.mtu > MAX_PACKET_BODY {
            return Err(field("mtu", format!("must be {MIN_MTU}-{MAX_PACKET_BODY}")));
        }
        Ok(())
    }
}
