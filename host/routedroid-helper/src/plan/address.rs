//! Whether an address can be a phone's on a LAN: inside one of the LAN
//! interface's subnets, a unicast host address there, and not already used
//! by the host, a gateway, a neighbour or a route.

use std::net::Ipv4Addr;

use anyhow::{Result, bail, ensure};
use routedroid_helper_ipc::IfName;

use super::Facts;
use crate::kernel::Address;
use crate::policy::mask;

/// The host address on `lan` whose subnet holds `phone_ip`, if `phone_ip`
/// may be a phone's there.
pub fn check(phone_ip: Ipv4Addr, lan_if: &IfName, lan: u32, facts: &Facts) -> Result<Address> {
    let subnet = |a: &Address| u32::from(a.addr) & mask(a.prefix);
    let Some(host) = facts
        .addresses
        .iter()
        .find(|a| a.index == lan && subnet(a) == u32::from(phone_ip) & mask(a.prefix))
    else {
        bail!("{phone_ip} is not inside any IPv4 subnet of {lan_if}");
    };
    let host_part = u32::from(phone_ip) & !mask(host.prefix);
    if host.prefix < 31 && (host_part == 0 || host_part == !mask(host.prefix)) {
        bail!(
            "{phone_ip} is the network or broadcast address of {}/{}",
            host.addr,
            host.prefix
        );
    }
    if phone_ip.is_loopback()
        || phone_ip.is_link_local()
        || phone_ip.is_multicast()
        || phone_ip.is_broadcast()
    {
        bail!("{phone_ip} is not a unicast LAN address");
    }
    ensure!(
        !facts.addresses.iter().any(|a| a.addr == phone_ip),
        "{phone_ip} is one of this host's addresses"
    );
    ensure!(
        !facts.routes.iter().any(|r| r.gateway == Some(phone_ip)),
        "{phone_ip} is a gateway"
    );
    ensure!(
        !facts.neighbours.contains(&phone_ip),
        "{phone_ip} is in use on {lan_if}"
    );
    let routed = facts
        .routes
        .iter()
        .any(|r| r.prefix == 32 && r.dst == phone_ip);
    ensure!(!routed, "{phone_ip} already has a host route");
    Ok(*host)
}

/// Addresses a lease must not be: the ones [`check`] refuses by identity,
/// so the DHCP client declines them before probing.
pub fn exclusions(facts: &Facts) -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = facts.addresses.iter().map(|a| a.addr).collect();
    out.extend(facts.routes.iter().filter_map(|r| r.gateway));
    out.extend(facts.neighbours.iter().copied());
    out.extend(
        facts
            .routes
            .iter()
            .filter(|r| r.prefix == 32)
            .map(|r| r.dst),
    );
    out.sort_unstable();
    out.dedup();
    out
}
