//! What a `Start` will do, decided before anything is touched: the request
//! is checked against the operator's policy and the kernel's current state,
//! and every refusal happens here, with a reason, not halfway through.

use std::net::Ipv4Addr;

use anyhow::{Result, bail, ensure};
use routedroid_dhcp::Held;
use routedroid_helper_ipc::IfName;

use crate::journal::Reservation;
use crate::kernel::{Egress, Firewall};
use crate::op::{Leaf, Op};
use crate::policy::Policy;
use crate::session_id::SessionId;
use crate::survey::unsuitable;

mod address;
mod egress;
mod facts;

pub use address::exclusions;
pub use facts::Facts;

pub use routedroid_helper_ipc::MTU_RANGE;
pub use routedroid_helper_ipc::TUN_PREFIX;

/// The controller's `Start`, already type-checked by the IPC layer, with
/// the phone's address settled: the requested one, or the leased one with
/// its lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub lan_if: IfName,
    pub phone_ip: Ipv4Addr,
    pub tun: IfName,
    pub mtu: u32,
    pub lease: Option<Held>,
    /// The lease's router: the phone's gateway, if it is on the LAN.
    pub router: Option<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    session: SessionId,
    request: Request,
    host_ip: Ipv4Addr,
    lan_prefix: u8,
    egress: Egress,
}

impl Plan {
    pub fn build(
        session: SessionId,
        request: Request,
        policy: &Policy,
        facts: &Facts,
    ) -> Result<Self> {
        let Request {
            lan_if,
            phone_ip,
            tun,
            mtu,
            lease,
            router,
        } = &request;
        let phone_ip = *phone_ip;
        ensure!(
            tun.as_str().starts_with(TUN_PREFIX),
            "TUN name must start with {TUN_PREFIX:?}"
        );
        ensure!(tun != lan_if, "TUN and LAN interface are the same");
        ensure!(
            MTU_RANGE.contains(mtu),
            "MTU {mtu} is outside {MTU_RANGE:?}"
        );
        match lease {
            None => policy.check(lan_if, phone_ip)?,
            Some(held) => {
                ensure!(
                    held.address == phone_ip && held.iface == lan_if.as_str(),
                    "the lease is for {} on {}, not {phone_ip} on {lan_if}",
                    held.address,
                    held.iface
                );
                policy.check_leased(lan_if, phone_ip)?;
            }
        }
        let Some(lan) = &facts.lan else {
            bail!("{lan_if} does not exist")
        };
        if let Some(reason) = unsuitable(lan, &[]) {
            bail!("{lan_if} cannot carry phones: {reason}");
        }
        ensure!(!facts.tun_exists, "{tun} already exists");
        let host = address::check(phone_ip, lan_if, lan.index, facts)?;
        let egress = egress::plan(phone_ip, *router, lan.index, &host, facts)?;

        Ok(Self {
            session,
            host_ip: host.addr,
            lan_prefix: host.prefix,
            egress,
            request,
        })
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

    /// The phone's gateway on the LAN; `None` confines it to the LAN.
    pub fn gateway(&self) -> Option<Ipv4Addr> {
        self.egress.gateway
    }

    pub fn reservation(&self) -> Reservation {
        Reservation {
            tun: self.request.tun.clone(),
            phone_ip: self.request.phone_ip,
        }
    }

    pub fn firewall(&self) -> Firewall {
        let Request {
            lan_if,
            phone_ip,
            tun,
            ..
        } = self.request.clone();
        Firewall {
            tun,
            lan_if,
            phone_ip,
            host_ip: self.host_ip,
            tag: self.session.tag(),
        }
    }

    /// Mutations in application order. Deny-first: the firewall exists before
    /// anything forwards, the phone's egress before it can send, and the
    /// route that attracts traffic comes last.
    /// A held lease comes first, so it is given back last.
    pub fn ops(&self) -> Vec<Op> {
        let Request {
            lan_if,
            phone_ip,
            tun,
            lease,
            ..
        } = &self.request;
        let sysctl = |ifname: &IfName, leaf| Op::Sysctl {
            ifname: ifname.clone(),
            leaf,
        };
        let lease = lease.iter().map(|held| Op::Lease {
            lan_if: lan_if.clone(),
            client_id: held.client_id.clone(),
            address: held.address,
            server_id: held.server_id,
            server_mac: held.server_mac.clone(),
        });
        lease
            .chain([
                Op::Tun { name: tun.clone() },
                Op::NftTable { tun: tun.clone() },
                sysctl(tun, Leaf::Forwarding),
                sysctl(lan_if, Leaf::Forwarding),
                sysctl(lan_if, Leaf::ProxyArp),
                Op::Egress {
                    phone_ip: *phone_ip,
                    lan_if: lan_if.clone(),
                    table: self.egress.table,
                    lan_net: self.egress.lan_net,
                    prefix: self.egress.prefix,
                    gateway: self.egress.gateway,
                },
                Op::Route {
                    dst: *phone_ip,
                    tun: tun.clone(),
                    src: self.host_ip,
                },
            ])
            .collect()
    }
}

#[cfg(test)]
mod tests;
