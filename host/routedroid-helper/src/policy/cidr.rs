//! `a.b.c.d/len` IPv4 blocks, written without host bits.

use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use anyhow::{bail, Context};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Cidr {
    network: Ipv4Addr,
    len: u8,
}

impl Cidr {
    pub fn network(&self) -> Ipv4Addr {
        self.network
    }

    pub fn prefix(&self) -> u8 {
        self.len
    }

    pub fn contains(&self, addr: Ipv4Addr) -> bool {
        u32::from(addr) & mask(self.len) == u32::from(self.network)
    }
}

pub fn mask(len: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(len)).unwrap_or(0)
}

impl FromStr for Cidr {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (addr, len) = s
            .split_once('/')
            .with_context(|| format!("{s:?} is not a.b.c.d/len"))?;
        let network: Ipv4Addr = addr
            .parse()
            .with_context(|| format!("{s:?}: bad address"))?;
        let len: u8 = len
            .parse()
            .ok()
            .filter(|l| *l <= 32)
            .with_context(|| format!("{s:?}: bad prefix length"))?;
        if u32::from(network) & !mask(len) != 0 {
            bail!("{s:?} has host bits set");
        }
        Ok(Self { network, len })
    }
}

impl TryFrom<String> for Cidr {
    type Error = anyhow::Error;

    fn try_from(s: String) -> anyhow::Result<Self> {
        s.parse()
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership() {
        let block: Cidr = "192.168.1.200/29".parse().unwrap();
        assert!(block.contains("192.168.1.200".parse().unwrap()));
        assert!(block.contains("192.168.1.207".parse().unwrap()));
        assert!(!block.contains("192.168.1.208".parse().unwrap()));
        let host: Cidr = "10.0.0.5/32".parse().unwrap();
        assert!(
            host.contains("10.0.0.5".parse().unwrap())
                && !host.contains("10.0.0.4".parse().unwrap())
        );
        assert!("0.0.0.0/0"
            .parse::<Cidr>()
            .unwrap()
            .contains("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn typos_are_refused() {
        for bad in [
            "192.168.1.201/29",
            "192.168.1.0",
            "192.168.1.0/33",
            "192.168.1/24",
            "x/8",
            "10.0.0.0/-1",
        ] {
            assert!(bad.parse::<Cidr>().is_err(), "{bad}");
        }
    }
}
