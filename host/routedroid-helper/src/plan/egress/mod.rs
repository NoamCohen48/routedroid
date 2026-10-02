//! Where the phone's own traffic goes: its table holds the LAN's subnet and
//! a gateway on that LAN, if there is one (the lease's router, else the
//! LAN interface's own default route); with none the phone reaches the LAN
//! only. The table is the phone's address as a number, and must not be
//! anyone else's: Routedroid's own (protocol 82) is a session's, which the
//! journal's address reservation already refuses or recovery removes.

use std::net::Ipv4Addr;

use anyhow::{Result, ensure};

use super::Facts;
use crate::kernel::{Address, Egress, MAIN_TABLE, ROUTE_PROTOCOL};
use crate::policy::mask;

pub fn plan(
    phone_ip: Ipv4Addr,
    router: Option<Ipv4Addr>,
    lan: u32,
    host: &Address,
    facts: &Facts,
) -> Result<Egress> {
    let table = u32::from(phone_ip);
    let foreign = |protocol| protocol != ROUTE_PROTOCOL;
    ensure!(
        !facts
            .routes
            .iter()
            .any(|r| r.table == table && foreign(r.protocol))
            && !facts
                .rules
                .iter()
                .any(|r| r.table == table && foreign(r.protocol)),
        "routing table {table} (the number of {phone_ip}) is already in use"
    );
    ensure!(
        !facts
            .rules
            .iter()
            .any(|r| r.src == Some((phone_ip, 32)) && foreign(r.protocol)),
        "a routing rule already selects traffic from {phone_ip}"
    );
    let net = u32::from(host.addr) & mask(host.prefix);
    let on_lan = |g: &Ipv4Addr| u32::from(*g) & mask(host.prefix) == net && *g != phone_ip;
    let lan_default = || {
        facts
            .routes
            .iter()
            .filter(|r| r.table == MAIN_TABLE && r.prefix == 0 && r.oif == Some(lan))
            .find_map(|r| r.gateway.filter(on_lan))
    };
    Ok(Egress {
        phone_ip,
        table,
        lan_net: Ipv4Addr::from(net),
        prefix: host.prefix,
        gateway: router.filter(on_lan).or_else(lan_default),
    })
}

#[cfg(test)]
mod tests;
