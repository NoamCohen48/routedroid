//! Everything the helper asks of the kernel, as one trait: the production
//! [`System`] speaks rtnetlink, `nft` and `/proc/sys`; tests use `Fake`.
//! The trait is mechanism only. Which object belongs to which session, and
//! what to refuse, is decided by the callers (`plan`, `session`, `claims`).
//!
//! Every query returns `Result`: "cannot tell" is an error, never "absent".

use std::net::Ipv4Addr;

use anyhow::Result;
use routedroid_helper_ipc::IfName;

use crate::op::SysctlKey;

mod system;

#[cfg(test)]
pub mod fake;

pub use system::{AsyncTun, System};

/// Routedroid's rtnetlink route protocol number (`proto 82` in `ip route`),
/// outside the values iproute2 names in `rt_protos`.
pub const ROUTE_PROTOCOL: u8 = 82;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub index: u32,
    pub alias: Option<String>,
}

/// One IPv4 address the host owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address {
    pub index: u32,
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

/// One IPv4 route, from any table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub dst: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Ipv4Addr>,
    pub oif: Option<u32>,
    pub protocol: u8,
}

/// The session's `/32` towards the phone. Installed with [`ROUTE_PROTOCOL`];
/// deletion matches destination, device, source and protocol exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRoute {
    pub dst: Ipv4Addr,
    pub oif: u32,
    pub src: Ipv4Addr,
}

impl HostRoute {
    pub fn matches(&self, route: &Route) -> bool {
        route.dst == self.dst
            && route.prefix == 32
            && route.oif == Some(self.oif)
            && route.protocol == ROUTE_PROTOCOL
    }
}

/// What the session firewall is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firewall {
    pub tun: IfName,
    pub lan_if: IfName,
    pub phone_ip: Ipv4Addr,
    pub host_ip: Ipv4Addr,
    /// Written as the table's comment: the owner tag.
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NftTable {
    /// Unique for the network namespace's lifetime, unlike the name.
    pub handle: u64,
    pub comment: Option<String>,
}

pub trait Kernel: Send + Sync + 'static {
    /// Owns the TUN: the interface exists while this value does.
    type Tun: Send + 'static;

    fn link(&self, name: &IfName) -> Result<Option<Link>>;
    fn addresses(&self) -> Result<Vec<Address>>;
    /// Addresses the kernel currently believes are on-link neighbours of `index`.
    fn neighbours(&self, index: u32) -> Result<Vec<Ipv4Addr>>;
    fn routes(&self) -> Result<Vec<Route>>;

    /// A new TUN (fails if the name exists), alias-tagged, with `mtu`, up.
    fn create_tun(&self, name: &IfName, alias: &str, mtu: u32) -> Result<Self::Tun>;
    fn delete_link(&self, index: u32) -> Result<()>;

    /// Fails if any route for the destination already exists (no replace).
    fn add_route(&self, route: &HostRoute) -> Result<()>;
    fn delete_route(&self, route: &HostRoute) -> Result<()>;

    /// Create `inet routedroid_<tun>` atomically; fails if the table exists.
    fn create_firewall(&self, firewall: &Firewall) -> Result<()>;
    fn nft_table(&self, name: &str) -> Result<Option<NftTable>>;
    fn delete_nft_table(&self, handle: u64) -> Result<()>;

    /// `None` when the interface does not exist (any more).
    fn sysctl_read(&self, key: &SysctlKey) -> Result<Option<String>>;
    fn sysctl_write(&self, key: &SysctlKey, value: &str) -> Result<()>;
}
