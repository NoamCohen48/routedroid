//! The production [`Kernel`]: rtnetlink for links, addresses, neighbours,
//! routes and rules; the `nft` binary for the firewall; `/proc/sys` for sysctls.

use std::net::Ipv4Addr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use routedroid_dhcp::Held;
use routedroid_helper_ipc::IfName;

use super::{Address, Egress, Firewall, HostRoute, Kernel, Link, NftTable, Route, Rule};
use crate::op::SysctlKey;

mod command;
mod egress;
mod link;
mod neighbourhood;
mod netlink;
mod nft;
mod route;
mod rule;
mod ruleset;
mod sysctl;
mod tun;

pub use tun::{AsyncTun, Device};

pub struct System {
    nft: PathBuf,
}

impl System {
    pub fn new() -> Result<Self> {
        Ok(Self {
            nft: command::locate(nft::CANDIDATES).context("find nft")?,
        })
    }
}

impl Kernel for System {
    type Tun = Device;

    fn link(&self, name: &IfName) -> Result<Option<Link>> {
        link::find(name)
    }

    fn links(&self) -> Result<Vec<Link>> {
        link::all()
    }

    fn addresses(&self) -> Result<Vec<Address>> {
        neighbourhood::addresses()
    }

    fn neighbours(&self, index: u32) -> Result<Vec<Ipv4Addr>> {
        neighbourhood::neighbours(index)
    }

    fn routes(&self) -> Result<Vec<Route>> {
        route::all()
    }

    fn rules(&self) -> Result<Vec<Rule>> {
        rule::all()
    }

    fn create_tun(&self, name: &IfName, alias: &str, mtu: u32) -> Result<Device> {
        let device = Device::create(name)?;
        // The fd keeps the name ours, so this lookup cannot find someone else's link.
        let index = link::find(name)?
            .with_context(|| format!("TUN {name} vanished after creation"))?
            .index;
        link::configure(index, alias, mtu)?;
        Ok(device)
    }

    fn delete_link(&self, index: u32) -> Result<()> {
        link::delete(index)
    }

    fn add_route(&self, route: &HostRoute) -> Result<()> {
        route::add(route)
    }

    fn delete_route(&self, route: &HostRoute) -> Result<()> {
        route::delete(route)
    }

    fn add_egress(&self, egress: &Egress, lan_index: u32) -> Result<()> {
        egress::add(egress, lan_index)
    }

    fn delete_egress(&self, egress: &Egress) -> Result<()> {
        egress::delete(egress)
    }

    fn create_firewall(&self, firewall: &Firewall) -> Result<()> {
        nft::create(&self.nft, firewall)
    }

    fn nft_table(&self, name: &str) -> Result<Option<NftTable>> {
        nft::find(&self.nft, name)
    }

    fn delete_nft_table(&self, handle: u64) -> Result<()> {
        nft::delete(&self.nft, handle)
    }

    fn release_lease(&self, lease: &Held) -> Result<()> {
        routedroid_dhcp::release_now(lease)
    }

    fn sysctl_read(&self, key: &SysctlKey) -> Result<Option<String>> {
        sysctl::read(key)
    }

    fn sysctl_write(&self, key: &SysctlKey, value: &str) -> Result<()> {
        sysctl::write(key, value)
    }
}
