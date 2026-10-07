//! `a.b.c.d/len` blocks of phone addresses, as the helper's policy takes
//! them: written without host bits, and inside the LAN they are for.

use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use routedroid_helper_ipc::Interface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub network: Ipv4Addr,
    pub prefix: u8,
}

fn mask(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

impl Block {
    /// Whether the block lies inside one of `link`'s subnets, so its
    /// addresses are on that LAN.
    pub fn inside(&self, link: &Interface) -> bool {
        link.addresses.iter().any(|net| {
            let mask = mask(net.prefix);
            self.prefix >= net.prefix
                && u32::from(self.network) & mask == u32::from(net.address) & mask
        })
    }

    /// A block to offer as an example on `link`: eight addresses at .200 of
    /// its first subnet, or the subnet itself when that is smaller.
    pub fn example(link: &Interface) -> Option<Self> {
        let net = link.addresses.first()?;
        let base = u32::from(net.address) & mask(net.prefix);
        Some(if net.prefix <= 24 {
            Self {
                network: Ipv4Addr::from(u32::from(net.address) & mask(24) | 200),
                prefix: 29,
            }
        } else {
            Self {
                network: Ipv4Addr::from(base),
                prefix: net.prefix,
            }
        })
    }
}

impl FromStr for Block {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (address, prefix) = s
            .trim()
            .split_once('/')
            .ok_or_else(|| format!("{s:?} is not a block like 192.168.1.200/29"))?;
        let network: Ipv4Addr = address
            .parse()
            .map_err(|_| format!("{address:?} is not an IPv4 address"))?;
        let prefix: u8 = prefix
            .parse()
            .ok()
            .filter(|prefix| *prefix <= 32)
            .ok_or_else(|| format!("{prefix:?} is not a prefix length (0 to 32)"))?;
        if u32::from(network) & !mask(prefix) != 0 {
            let start = Ipv4Addr::from(u32::from(network) & mask(prefix));
            return Err(format!(
                "{s} does not start a block: did you mean {start}/{prefix}?"
            ));
        }
        Ok(Self { network, prefix })
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}
