//! An in-memory [`Kernel`] for unit tests. It keeps the semantics the helper
//! relies on: TUN names are exclusive, a TUN's routes and sysctls vanish
//! with its fd, routes are never replaced, tables are never merged, and any
//! call can be made to fail.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, bail};
use routedroid_dhcp::Held;
use routedroid_helper_ipc::IfName;

use super::{
    Address, Egress, Firewall, ForwardDrop, HostRoute, Kernel, Link, LinkKind, NftTable, Route,
    Rule,
};
use crate::op::{SysctlKey, nft_table_name};

mod routing;
mod state;

pub use state::State;

#[derive(Clone, Default)]
pub struct Fake(Arc<Mutex<State>>);

pub struct FakeTun {
    state: Fake,
    index: u32,
}

impl Drop for FakeTun {
    fn drop(&mut self) {
        self.state.lock().remove_link(self.index);
    }
}

impl Fake {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Everything observable, for before/after comparisons.
    pub fn snapshot(&self) -> String {
        let s = self.lock();
        let sysctls: Vec<_> = s
            .sysctls
            .iter()
            .filter(|(_, value)| *value != "0")
            .collect();
        format!(
            "links={:?}\nroutes={:?}\nrules={:?}\ntables={:?}\nsysctls={sysctls:?}",
            s.links, s.routes, s.rules, s.tables
        )
    }

    fn call(&self, name: &'static str) -> Result<MutexGuard<'_, State>> {
        let state = self.lock();
        if state.failing.contains(name) {
            bail!("fake kernel: {name} fails");
        }
        Ok(state)
    }
}

impl Kernel for Fake {
    type Tun = FakeTun;

    fn link(&self, name: &IfName) -> Result<Option<Link>> {
        Ok(self.call("link")?.links.get(name.as_str()).cloned())
    }

    fn links(&self) -> Result<Vec<Link>> {
        Ok(self.call("links")?.links.values().cloned().collect())
    }

    fn addresses(&self) -> Result<Vec<Address>> {
        Ok(self.call("addresses")?.addresses.clone())
    }

    fn neighbours(&self, index: u32) -> Result<Vec<Ipv4Addr>> {
        let state = self.call("neighbours")?;
        Ok(state
            .neighbours
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, a)| *a)
            .collect())
    }

    fn routes(&self) -> Result<Vec<Route>> {
        Ok(self.call("routes")?.routes.clone())
    }

    fn rules(&self) -> Result<Vec<Rule>> {
        Ok(self.call("rules")?.rules.clone())
    }

    fn create_tun(&self, name: &IfName, alias: &str, _mtu: u32) -> Result<FakeTun> {
        let mut state = self.call("create_tun")?;
        if state.links.contains_key(name.as_str()) {
            bail!("create TUN {name}: EBUSY");
        }
        let index = state.add_link(name.as_str(), Some(alias));
        state.links.get_mut(name.as_str()).expect("just added").kind = LinkKind::TunTap;
        Ok(FakeTun {
            state: self.clone(),
            index,
        })
    }

    fn delete_link(&self, index: u32) -> Result<()> {
        self.call("delete_link")?.remove_link(index);
        Ok(())
    }

    fn add_route(&self, route: &HostRoute) -> Result<()> {
        self.call("add_route")?.add_route(route)
    }

    fn delete_route(&self, route: &HostRoute) -> Result<()> {
        self.call("delete_route")?.delete_route(route)
    }

    fn add_egress(&self, egress: &Egress, lan_index: u32) -> Result<()> {
        self.call("add_egress")?.add_egress(egress, lan_index)
    }

    fn delete_egress(&self, egress: &Egress) -> Result<()> {
        self.call("delete_egress")?.delete_egress(egress);
        Ok(())
    }

    fn create_firewall(&self, firewall: &Firewall) -> Result<()> {
        let mut state = self.call("create_firewall")?;
        let name = nft_table_name(&firewall.tun);
        if state.tables.contains_key(&name) {
            bail!("create table inet {name}: EEXIST");
        }
        state.next_handle += 1;
        let table = NftTable {
            handle: state.next_handle,
            comment: Some(firewall.tag.clone()),
        };
        state.tables.insert(name, (table, firewall.clone()));
        Ok(())
    }

    fn nft_table(&self, name: &str) -> Result<Option<NftTable>> {
        Ok(self
            .call("nft_table")?
            .tables
            .get(name)
            .map(|(table, _)| table.clone()))
    }

    fn nft_tables(&self) -> Result<Vec<(String, NftTable)>> {
        let state = self.call("nft_tables")?;
        Ok(state
            .tables
            .iter()
            .map(|(name, (table, _))| (name.clone(), table.clone()))
            .collect())
    }

    fn forward_drops(&self) -> Result<Vec<ForwardDrop>> {
        Ok(self.call("forward_drops")?.forward_drops.clone())
    }

    fn delete_nft_table(&self, handle: u64) -> Result<()> {
        let mut state = self.call("delete_nft_table")?;
        let Some(name) = state
            .tables
            .iter()
            .find(|(_, (t, _))| t.handle == handle)
            .map(|(n, _)| n.clone())
        else {
            bail!("delete table handle {handle}: ENOENT");
        };
        state.tables.remove(&name);
        Ok(())
    }

    fn sysctl_read(&self, key: &SysctlKey) -> Result<Option<String>> {
        let state = self.call("sysctl_read")?;
        if !state.links.contains_key(key.ifname.as_str()) {
            return Ok(None);
        }
        Ok(Some(
            state
                .sysctls
                .get(key)
                .cloned()
                .unwrap_or_else(|| "0".into()),
        ))
    }

    fn release_lease(&self, lease: &Held) -> Result<()> {
        self.call("release_lease")?.released.push(lease.clone());
        Ok(())
    }

    fn sysctl_write(&self, key: &SysctlKey, value: &str) -> Result<()> {
        let mut state = self.call("sysctl_write")?;
        if !state.links.contains_key(key.ifname.as_str()) {
            bail!("write {key}: ENOENT");
        }
        state.sysctls.insert(key.clone(), value.to_owned());
        Ok(())
    }
}
