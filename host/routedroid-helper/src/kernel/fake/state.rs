//! The fake kernel's objects, and the one side effect that spans them:
//! removing a link takes its routes and sysctls with it.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use crate::kernel::{Address, Firewall, Link, LinkKind, NftTable, Route};
use crate::op::SysctlKey;

#[derive(Default)]
pub struct State {
    pub links: BTreeMap<String, Link>,
    pub addresses: Vec<Address>,
    pub neighbours: Vec<(u32, Ipv4Addr)>,
    pub routes: Vec<Route>,
    pub tables: BTreeMap<String, (NftTable, Firewall)>,
    /// Values of existing interfaces' keys; unset ones read as "0".
    pub sysctls: BTreeMap<SysctlKey, String>,
    /// Names of trait methods that fail until removed.
    pub failing: BTreeSet<&'static str>,
    next_index: u32,
    pub(super) next_handle: u64,
}

impl State {
    /// An Ethernet link, up, with carrier.
    pub fn add_link(&mut self, name: &str, alias: Option<&str>) -> u32 {
        self.next_index += 1;
        let link = Link {
            index: self.next_index,
            name: name.to_owned(),
            alias: alias.map(str::to_owned),
            kind: LinkKind::Ethernet,
            up: true,
            carrier: true,
            master: None,
        };
        self.links.insert(name.to_owned(), link);
        self.next_index
    }

    pub(super) fn remove_link(&mut self, index: u32) {
        let Some(name) = self
            .links
            .iter()
            .find(|(_, l)| l.index == index)
            .map(|(n, _)| n.clone())
        else {
            return;
        };
        self.links.remove(&name);
        self.routes.retain(|r| r.oif != Some(index));
        self.sysctls.retain(|key, _| key.ifname.as_str() != name);
    }
}
