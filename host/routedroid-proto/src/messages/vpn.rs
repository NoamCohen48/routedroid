//! CONFIGURE_VPN and VPN_READY (§4.4–4.5).

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefix {
    pub address: String,
    pub prefix: u8,
}

impl Prefix {
    pub fn new(address: Ipv4Addr, prefix: u8) -> Self {
        Self { address: address.to_string(), prefix }
    }

    fn check(&self, name: &'static str) -> Result<(), BodyError> {
        check_ipv4(name, &self.address)?;
        if self.prefix > 32 {
            return Err(field(name, "prefix must be 0-32"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigureVpn {
    pub mtu: u32,
    pub addresses: Vec<Prefix>,
    pub routes: Vec<Prefix>,
    pub dns: Vec<String>,
    pub session_name: String,
}

impl Body for ConfigureVpn {
    fn validate(&self) -> Result<(), BodyError> {
        if self.mtu < MIN_MTU || self.mtu > MAX_PACKET_BODY {
            return Err(field("mtu", format!("must be {MIN_MTU}-{MAX_PACKET_BODY}")));
        }
        if self.addresses.len() != 1 {
            return Err(field("addresses", "exactly one address in version 1"));
        }
        for a in &self.addresses {
            a.check("addresses")?;
        }
        if self.routes.is_empty() {
            return Err(field("routes", "at least one route"));
        }
        for r in &self.routes {
            r.check("routes")?;
        }
        for d in &self.dns {
            check_ipv4("dns", d)?;
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
            if prefix.parse::<u8>().map_or(true, |p| p > 32) {
                return Err(field("addresses", "prefix must be 0-32"));
            }
        }
        if self.mtu < MIN_MTU || self.mtu > MAX_PACKET_BODY {
            return Err(field("mtu", format!("must be {MIN_MTU}-{MAX_PACKET_BODY}")));
        }
        Ok(())
    }
}
