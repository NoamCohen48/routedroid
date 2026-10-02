//! `Interfaces`: every link the host has, and whether a phone may join the
//! LAN through it. The structural rules are the ones `Start` enforces too
//! ([`unsuitable`]); the rest (carrier, IPv4, policy) say why a `Start` on
//! the link would be refused or pointless right now.

use anyhow::Result;
use routedroid_helper_ipc::{Interface, Net, TUN_PREFIX};

use crate::kernel::{Address, Kernel, Link, LinkKind};
use crate::policy::Policy;

/// Why `link` can never carry phones, if it can't. `links` names masters.
pub fn unsuitable(link: &Link, links: &[Link]) -> Option<String> {
    let reason = match link.kind {
        LinkKind::Loopback => "loopback".into(),
        _ if link.name.starts_with(TUN_PREFIX) => "a phone's TUN".into(),
        LinkKind::TunTap => "a TUN/TAP device".into(),
        LinkKind::Other => "not Ethernet (proxy ARP needs ARP)".into(),
        LinkKind::Ethernet => match link.master {
            Some(index) => match links.iter().find(|l| l.index == index) {
                Some(master) => format!("a port of {}; use that instead", master.name),
                None => "a port of a bridge or bond; use that instead".into(),
            },
            None if !link.up => "down".into(),
            None => return None,
        },
    };
    Some(reason)
}

/// The survey, in kernel index order. A policy that cannot be read makes
/// every link ineligible, with the reason, rather than failing the request.
pub fn survey(kernel: &impl Kernel, policy: &Result<Policy>) -> Result<Vec<Interface>> {
    let mut links = kernel.links()?;
    links.sort_by_key(|link| link.index);
    let addresses = kernel.addresses()?;
    let routes = kernel.routes()?;
    Ok(links
        .iter()
        .map(|link| {
            let addresses: Vec<Net> = addresses
                .iter()
                .filter(|a| a.index == link.index)
                .map(net)
                .collect();
            let phone_addresses = match policy {
                Ok(policy) => policy
                    .phone_addresses(&link.name)
                    .iter()
                    .map(|block| Net {
                        address: block.network(),
                        prefix: block.prefix(),
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            let ineligible = unsuitable(link, &links).or_else(|| {
                if !link.carrier {
                    Some("no carrier (cable or Wi-Fi down)".into())
                } else if addresses.is_empty() {
                    Some("no IPv4 address".into())
                } else if let Err(e) = policy {
                    Some(format!("the helper policy cannot be read: {e:#}"))
                } else if phone_addresses.is_empty() {
                    Some("not in the helper policy".into())
                } else {
                    None
                }
            });
            Interface {
                name: link.name.clone(),
                up: link.up && link.carrier,
                default_route: routes
                    .iter()
                    .any(|r| r.prefix == 0 && r.oif == Some(link.index)),
                addresses,
                phone_addresses,
                ineligible,
            }
        })
        .collect())
}

fn net(address: &Address) -> Net {
    Net {
        address: address.addr,
        prefix: address.prefix,
    }
}

#[cfg(test)]
mod tests;
