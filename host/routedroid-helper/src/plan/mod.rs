//! What a `Start` will do, decided before anything is touched: the request
//! is checked against the operator's policy and the kernel's current state,
//! and every refusal happens here, with a reason, not halfway through.

use std::net::Ipv4Addr;

use anyhow::{bail, ensure, Result};
use routedroid_helper_ipc::IfName;

use crate::journal::Reservation;
use crate::kernel::Firewall;
use crate::op::{Leaf, Op};
use crate::policy::{mask, Policy};
use crate::session_id::SessionId;

mod facts;

pub use facts::Facts;

pub const TUN_PREFIX: &str = "phone";
pub const MTU_RANGE: std::ops::RangeInclusive<u32> = 576..=9000;

/// The controller's `Start`, already type-checked by the IPC layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub lan_if: IfName,
    pub phone_ip: Ipv4Addr,
    pub tun: IfName,
    pub mtu: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    session: SessionId,
    request: Request,
    host_ip: Ipv4Addr,
    lan_prefix: u8,
}

impl Plan {
    pub fn build(session: SessionId, request: Request, policy: &Policy, facts: &Facts) -> Result<Self> {
        let Request { lan_if, phone_ip, tun, mtu } = &request;
        let phone_ip = *phone_ip;
        ensure!(tun.as_str().starts_with(TUN_PREFIX), "TUN name must start with {TUN_PREFIX:?}");
        ensure!(tun != lan_if, "TUN and LAN interface are the same");
        ensure!(MTU_RANGE.contains(mtu), "MTU {mtu} is outside {MTU_RANGE:?}");
        policy.check(lan_if, phone_ip)?;
        let Some(lan) = facts.lan else { bail!("{lan_if} does not exist") };
        ensure!(!facts.tun_exists, "{tun} already exists");

        let subnet = |a: &crate::kernel::Address| u32::from(a.addr) & mask(a.prefix);
        let Some(host) = facts
            .addresses
            .iter()
            .find(|a| a.index == lan && subnet(a) == u32::from(phone_ip) & mask(a.prefix))
        else {
            bail!("{phone_ip} is not inside any IPv4 subnet of {lan_if}");
        };
        let host_part = u32::from(phone_ip) & !mask(host.prefix);
        if host.prefix < 31 && (host_part == 0 || host_part == !mask(host.prefix)) {
            bail!("{phone_ip} is the network or broadcast address of {}/{}", host.addr, host.prefix);
        }
        if phone_ip.is_loopback() || phone_ip.is_link_local() || phone_ip.is_multicast() || phone_ip.is_broadcast() {
            bail!("{phone_ip} is not a unicast LAN address");
        }
        ensure!(!facts.addresses.iter().any(|a| a.addr == phone_ip), "{phone_ip} is one of this host's addresses");
        ensure!(!facts.routes.iter().any(|r| r.gateway == Some(phone_ip)), "{phone_ip} is a gateway");
        ensure!(!facts.neighbours.contains(&phone_ip), "{phone_ip} is in use on {lan_if}");
        let routed = facts.routes.iter().any(|r| r.prefix == 32 && r.dst == phone_ip);
        ensure!(!routed, "{phone_ip} already has a host route");

        Ok(Self { session, host_ip: host.addr, lan_prefix: host.prefix, request })
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    pub fn request(&self) -> &Request {
        &self.request
    }

    pub fn host_ip(&self) -> Ipv4Addr {
        self.host_ip
    }

    pub fn lan_prefix(&self) -> u8 {
        self.lan_prefix
    }

    pub fn reservation(&self) -> Reservation {
        Reservation { tun: self.request.tun.clone(), phone_ip: self.request.phone_ip }
    }

    pub fn firewall(&self) -> Firewall {
        let Request { lan_if, phone_ip, tun, .. } = self.request.clone();
        Firewall { tun, lan_if, phone_ip, host_ip: self.host_ip, tag: self.session.tag() }
    }

    /// Mutations in application order. Deny-first: the firewall exists before
    /// anything forwards, and the route that attracts traffic comes last.
    pub fn ops(&self) -> Vec<Op> {
        let Request { lan_if, phone_ip, tun, .. } = &self.request;
        let sysctl = |ifname: &IfName, leaf| Op::Sysctl { ifname: ifname.clone(), leaf };
        vec![
            Op::Tun { name: tun.clone() },
            Op::NftTable { tun: tun.clone() },
            sysctl(tun, Leaf::Forwarding),
            sysctl(lan_if, Leaf::Forwarding),
            sysctl(lan_if, Leaf::ProxyArp),
            Op::Route { dst: *phone_ip, tun: tun.clone(), src: self.host_ip },
        ]
    }
}

#[cfg(test)]
mod tests;
