//! A `start` request, checked: every field the connection will use, in types
//! that cannot hold a bad value, and every default filled in. It is built
//! before `started` is answered, so a refused start starts nothing.

use std::net::Ipv4Addr;
use std::time::Duration;

use routedroid_helper_ipc::{IfName, MTU_RANGE, TUN_PREFIX};
use routedroid_ipc::{DnsChoice, StartRequest};
use routedroid_proto::frame::DEFAULT_MTU;
use routedroid_proto::messages::is_unicast_host;

use crate::device::Transport;
use crate::fault::{Fault, Kind, Result};

/// How long the app has to dial in after launch, unless the client says.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
/// How long an unplugged phone's address is held for it, unless the client says.
pub const DEFAULT_RECONNECT_WAIT: Duration = Duration::from_secs(120);
/// Past this the operator surely meant something else (either wait).
const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
/// More DNS servers than any resolver tries; refused rather than truncated.
const MAX_DNS: usize = 4;

/// A start the daemon will attempt; only the TUN may still be unnamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSpec {
    pub serial: String,
    pub lan_if: IfName,
    /// `None` leases one from the LAN's DHCP server.
    pub phone_ip: Option<Ipv4Addr>,
    pub tun: Option<IfName>,
    pub mtu: u32,
    pub dns: DnsChoice,
    pub connect_timeout: Duration,
    /// Zero: an unplugged phone ends the connection.
    pub reconnect_wait: Duration,
}

fn usage(message: impl std::fmt::Display) -> Fault {
    Fault::msg(Kind::Usage, message)
}

impl StartSpec {
    /// `request` has its serial and LAN filled in (see `resolve`).
    pub fn parse(request: StartRequest) -> Result<Self> {
        let serial = request.serial.ok_or_else(|| usage("which phone?"))?;
        let lan_if = request
            .lan_if
            .ok_or_else(|| usage("which LAN interface?"))?;
        Transport::check(&serial, request.allow_network_adb)?;
        let lan_if = IfName::new(&lan_if).map_err(|e| usage(format!("lan_if: {e}")))?;
        if lan_if.as_str().starts_with(TUN_PREFIX) {
            return Err(usage(format!(
                "{lan_if} is a phone's TUN, not a LAN interface"
            )));
        }
        let tun = request.tun.as_deref().map(tun_name).transpose()?;
        let phone_ip = request.phone_ip;
        if let Some(ip) = phone_ip
            && !is_unicast_host(ip)
        {
            return Err(usage(format!("{ip} is not a unicast host address")));
        }
        let mtu = request.mtu.unwrap_or(DEFAULT_MTU);
        if !MTU_RANGE.contains(&mtu) {
            return Err(usage(format!("mtu {mtu} is outside {MTU_RANGE:?}")));
        }
        let connect_timeout = match request.connect_timeout_secs {
            None => DEFAULT_CONNECT_TIMEOUT,
            Some(0) => return Err(usage("connect timeout must be at least 1 s")),
            Some(secs) => Duration::from_secs(secs).min(MAX_WAIT),
        };
        let reconnect_wait = request
            .reconnect_secs
            .map_or(DEFAULT_RECONNECT_WAIT, |secs| {
                Duration::from_secs(secs).min(MAX_WAIT)
            });
        Ok(Self {
            serial,
            lan_if,
            phone_ip,
            tun,
            mtu,
            dns: dns(request.dns.unwrap_or_default())?,
            connect_timeout,
            reconnect_wait,
        })
    }
}

/// What one connection is, fixed for its life and shared by its handle and
/// its task.
#[derive(Debug)]
pub struct ConnectionSpec {
    pub serial: String,
    pub lan_if: IfName,
    /// The requested address; `None` while (and after) leasing one.
    pub phone_ip: Option<Ipv4Addr>,
    pub tun: IfName,
    pub mtu: u32,
    pub dns: DnsChoice,
    pub connect_timeout: Duration,
    pub reconnect_wait: Duration,
    /// Unix seconds when `start` was accepted.
    pub started_at: u64,
}

impl ConnectionSpec {
    pub fn new(start: StartSpec, tun: IfName) -> Self {
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Self {
            serial: start.serial,
            lan_if: start.lan_if,
            phone_ip: start.phone_ip,
            tun,
            mtu: start.mtu,
            dns: start.dns,
            connect_timeout: start.connect_timeout,
            reconnect_wait: start.reconnect_wait,
            started_at,
        }
    }
}

fn tun_name(name: &str) -> Result<IfName> {
    let tun = IfName::new(name).map_err(|e| usage(format!("tun: {e}")))?;
    if !tun.as_str().starts_with(TUN_PREFIX) {
        return Err(usage(format!(
            "TUN name {tun} must start with {TUN_PREFIX:?}"
        )));
    }
    Ok(tun)
}

fn dns(choice: DnsChoice) -> Result<DnsChoice> {
    if let DnsChoice::Servers(servers) = &choice {
        if servers.is_empty() {
            return Err(usage("an empty DNS list: ask for no DNS instead"));
        }
        if servers.len() > MAX_DNS {
            return Err(usage(format!("at most {MAX_DNS} DNS servers")));
        }
        if let Some(bad) = servers.iter().find(|dns| !is_unicast_host(**dns)) {
            return Err(usage(format!(
                "DNS server {bad} is not a unicast host address"
            )));
        }
    }
    Ok(choice)
}

#[cfg(test)]
mod tests;
