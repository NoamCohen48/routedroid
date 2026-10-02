//! Read-only rtnetlink queries about who is already on the network: the
//! host's own IPv4 addresses and the neighbour (ARP) table.

use std::net::{IpAddr, Ipv4Addr};

use anyhow::{Context, Result};
use netlink_packet_route::address::{AddressAttribute, AddressMessage};
use netlink_packet_route::neighbour::{
    NeighbourAddress, NeighbourAttribute, NeighbourMessage, NeighbourState,
};
use netlink_packet_route::{AddressFamily, RouteNetlinkMessage};

use super::netlink;
use crate::kernel::Address;

pub fn addresses() -> Result<Vec<Address>> {
    let mut request = AddressMessage::default();
    request.header.family = AddressFamily::Inet;
    let replies =
        netlink::dump(RouteNetlinkMessage::GetAddress(request)).context("dump addresses")?;
    let mut out = Vec::new();
    for reply in replies {
        let RouteNetlinkMessage::NewAddress(message) = reply else {
            continue;
        };
        // IFA_LOCAL is the host's own address; IFA_ADDRESS is the peer on point-to-point links.
        let local = message.attributes.iter().find_map(|a| match a {
            AddressAttribute::Local(IpAddr::V4(addr)) => Some(*addr),
            _ => None,
        });
        let address = message.attributes.iter().find_map(|a| match a {
            AddressAttribute::Address(IpAddr::V4(addr)) => Some(*addr),
            _ => None,
        });
        if let Some(addr) = local.or(address) {
            out.push(Address {
                index: message.header.index,
                addr,
                prefix: message.header.prefix_len,
            });
        }
    }
    Ok(out)
}

/// Neighbours of `index` that are, or recently were, answering. Failed and
/// still-incomplete entries are not evidence that anyone uses the address.
pub fn neighbours(index: u32) -> Result<Vec<Ipv4Addr>> {
    let mut request = NeighbourMessage::default();
    request.header.family = AddressFamily::Inet;
    let replies =
        netlink::dump(RouteNetlinkMessage::GetNeighbour(request)).context("dump neighbours")?;
    let mut out = Vec::new();
    for reply in replies {
        let RouteNetlinkMessage::NewNeighbour(message) = reply else {
            continue;
        };
        if message.header.ifindex != index || !occupied(message.header.state) {
            continue;
        }
        out.extend(message.attributes.iter().find_map(|a| match a {
            NeighbourAttribute::Destination(NeighbourAddress::Inet(addr)) => Some(*addr),
            _ => None,
        }));
    }
    Ok(out)
}

fn occupied(state: NeighbourState) -> bool {
    !matches!(
        state,
        NeighbourState::Failed | NeighbourState::Incomplete | NeighbourState::None
    )
}
