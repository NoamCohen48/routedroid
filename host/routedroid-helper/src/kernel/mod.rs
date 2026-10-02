//! Everything the helper asks of the kernel, as one trait: the production
//! [`System`] speaks rtnetlink, `nft` and `/proc/sys`; tests use `Fake`.
//! The trait is mechanism only. Which object belongs to which session, and
//! what to refuse, is decided by the callers (`plan`, `session`, `claims`).
//!
//! Every query returns `Result`: "cannot tell" is an error, never "absent".

use std::net::Ipv4Addr;

use anyhow::Result;
use routedroid_dhcp::Held;
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
    pub name: String,
    pub alias: Option<String>,
    pub kind: LinkKind,
    /// Administratively up (`IFF_UP`).
    pub up: bool,
    /// The lower layer is up (`IFF_LOWER_UP`): a cable, an association.
    pub carrier: bool,
    /// The bridge or bond this link is a port of.
    pub master: Option<u32>,
}

/// What a link is, as far as carrying a phone goes: proxy ARP needs a link
/// that speaks ARP, which TUN/TAP devices and loopback do not usefully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Loopback,
    Ethernet,
    TunTap,
    /// Any other link layer (WireGuard, PPP, CAN, ...).
    Other,
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
    pub table: u32,
    pub dst: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Ipv4Addr>,
    pub oif: Option<u32>,
    pub protocol: u8,
}

/// The kernel's main routing table (`RT_TABLE_MAIN`).
pub const MAIN_TABLE: u32 = 254;

/// One IPv4 policy-routing rule (`ip rule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub priority: u32,
    pub table: u32,
    /// `from src/src_len`; `None` matches every source.
    pub src: Option<(Ipv4Addr, u8)>,
    pub protocol: u8,
}

/// Where a phone's own traffic is routed: rule `from phone_ip/32 lookup
/// table` at [`EGRESS_PRIORITY`], and in `table` only the LAN (its subnet
/// and, if known, its gateway) over an `unreachable` default at the highest
/// metric, so nothing the phone sends follows another interface's route,
/// not even once the LAN's own routes are gone. Rule and routes carry
/// [`ROUTE_PROTOCOL`]; undo removes exactly those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Egress {
    pub phone_ip: Ipv4Addr,
    pub table: u32,
    pub lan_net: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Ipv4Addr>,
}

/// Ahead of `main` (32766) and of the rules VPN clients install in the
/// thousands, behind `local` (0), so the host's own addresses stay local.
pub const EGRESS_PRIORITY: u32 = 1082;

impl Egress {
    /// The rule this egress installs.
    pub fn rule(&self) -> Rule {
        Rule {
            priority: EGRESS_PRIORITY,
            table: self.table,
            src: Some((self.phone_ip, 32)),
            protocol: ROUTE_PROTOCOL,
        }
    }

    /// Whether `route` is one of this egress's own.
    pub fn owns(&self, route: &Route) -> bool {
        route.table == self.table && route.protocol == ROUTE_PROTOCOL
    }
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
    fn links(&self) -> Result<Vec<Link>>;
    fn addresses(&self) -> Result<Vec<Address>>;
    /// Addresses the kernel currently believes are on-link neighbours of `index`.
    fn neighbours(&self, index: u32) -> Result<Vec<Ipv4Addr>>;
    fn routes(&self) -> Result<Vec<Route>>;
    fn rules(&self) -> Result<Vec<Rule>>;

    /// A new TUN (fails if the name exists), alias-tagged, with `mtu`, up.
    fn create_tun(&self, name: &IfName, alias: &str, mtu: u32) -> Result<Self::Tun>;
    fn delete_link(&self, index: u32) -> Result<()>;

    /// Fails if any route for the destination already exists (no replace).
    fn add_route(&self, route: &HostRoute) -> Result<()>;
    fn delete_route(&self, route: &HostRoute) -> Result<()>;

    /// The table's routes (via `lan_index`), then the rule; fails if the
    /// rule or any of the routes exists.
    fn add_egress(&self, egress: &Egress, lan_index: u32) -> Result<()>;
    /// Remove the rule and every route [`Egress::owns`]; absent ones are fine.
    fn delete_egress(&self, egress: &Egress) -> Result<()>;

    /// Create `inet routedroid_<tun>` atomically; fails if the table exists.
    fn create_firewall(&self, firewall: &Firewall) -> Result<()>;
    fn nft_table(&self, name: &str) -> Result<Option<NftTable>>;
    fn delete_nft_table(&self, handle: u64) -> Result<()>;

    /// RELEASE a DHCP lease from its record. Fire and forget: the server
    /// sends no reply, and one that never hears it lets the lease expire.
    fn release_lease(&self, lease: &Held) -> Result<()>;

    /// `None` when the interface does not exist (any more).
    fn sysctl_read(&self, key: &SysctlKey) -> Result<Option<String>>;
    fn sysctl_write(&self, key: &SysctlKey, value: &str) -> Result<()>;
}
