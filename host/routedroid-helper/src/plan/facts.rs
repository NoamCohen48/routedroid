//! The kernel state a plan is judged against, read once, before any
//! mutation. The mutations themselves still fail on conflict (exclusive
//! TUN, `create table`, route add without replace), so a change between
//! this snapshot and the apply is refused, not overwritten.

use std::net::Ipv4Addr;

use anyhow::Result;
use routedroid_helper_ipc::IfName;

use crate::kernel::{Address, Kernel, Route};

#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The LAN interface's index, if it exists.
    pub lan: Option<u32>,
    pub tun_exists: bool,
    /// Every IPv4 address the host owns, on any interface.
    pub addresses: Vec<Address>,
    /// Every IPv4 route in every table.
    pub routes: Vec<Route>,
    /// Neighbours the LAN interface currently knows.
    pub neighbours: Vec<Ipv4Addr>,
}

impl Facts {
    pub fn gather(kernel: &impl Kernel, lan_if: &IfName, tun: &IfName) -> Result<Self> {
        let lan = kernel.link(lan_if)?.map(|link| link.index);
        Ok(Self {
            lan,
            tun_exists: kernel.link(tun)?.is_some(),
            addresses: kernel.addresses()?,
            routes: kernel.routes()?,
            neighbours: match lan {
                Some(index) => kernel.neighbours(index)?,
                None => Vec::new(),
            },
        })
    }
}
