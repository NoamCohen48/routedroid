//! IPv4 routes over rtnetlink: dump every table, add without replacing,
//! delete by exact match.

use std::net::Ipv4Addr;

use anyhow::{Context, Result};
use netlink_packet_core::{NLM_F_CREATE, NLM_F_EXCL};
use netlink_packet_route::route::{
    RouteAddress, RouteAttribute, RouteHeader, RouteMessage, RouteProtocol, RouteScope, RouteType,
};
use netlink_packet_route::{AddressFamily, RouteNetlinkMessage};

use super::netlink;
use crate::kernel::{HostRoute, ROUTE_PROTOCOL, Route};

/// Every IPv4 route in every table; a multipath route yields one entry per hop.
pub fn all() -> Result<Vec<Route>> {
    let mut request = RouteMessage::default();
    request.header.address_family = AddressFamily::Inet;
    let replies = netlink::dump(RouteNetlinkMessage::GetRoute(request)).context("dump routes")?;
    let mut out = Vec::new();
    for reply in replies {
        let RouteNetlinkMessage::NewRoute(message) = reply else {
            continue;
        };
        out.extend(entries(&message));
    }
    Ok(out)
}

fn entries(message: &RouteMessage) -> Vec<Route> {
    let mut route = Route {
        table: u32::from(message.header.table),
        dst: Ipv4Addr::UNSPECIFIED,
        prefix: message.header.destination_prefix_length,
        gateway: None,
        oif: None,
        protocol: message.header.protocol.into(),
    };
    let mut hops = Vec::new();
    for attribute in &message.attributes {
        match attribute {
            RouteAttribute::Destination(RouteAddress::Inet(dst)) => route.dst = *dst,
            RouteAttribute::Gateway(RouteAddress::Inet(gateway)) => route.gateway = Some(*gateway),
            RouteAttribute::Oif(oif) => route.oif = Some(*oif),
            // Tables past 255 only fit the attribute.
            RouteAttribute::Table(table) => route.table = *table,
            RouteAttribute::MultiPath(next_hops) => {
                hops.extend(next_hops.iter().map(|hop| Route {
                    gateway: hop.attributes.iter().find_map(|a| match a {
                        RouteAttribute::Gateway(RouteAddress::Inet(gateway)) => Some(*gateway),
                        _ => None,
                    }),
                    oif: Some(hop.interface_index),
                    ..route
                }));
            }
            _ => {}
        }
    }
    if hops.is_empty() {
        vec![route]
    } else {
        hops.into_iter()
            .map(|hop| Route {
                dst: route.dst,
                ..hop
            })
            .collect()
    }
}

pub fn add(route: &HostRoute) -> Result<()> {
    netlink::change(
        RouteNetlinkMessage::NewRoute(message(route)),
        NLM_F_CREATE | NLM_F_EXCL,
    )
    .with_context(|| format!("add route {}/32 via #{}", route.dst, route.oif))
}

pub fn delete(route: &HostRoute) -> Result<()> {
    netlink::change(RouteNetlinkMessage::DelRoute(message(route)), 0)
        .with_context(|| format!("delete route {}/32 via #{}", route.dst, route.oif))
}

/// The one shape both add and delete use, so deletion matches exactly what
/// was added: main table, our protocol, link scope, device and source.
fn message(route: &HostRoute) -> RouteMessage {
    let mut message = RouteMessage::default();
    message.header.address_family = AddressFamily::Inet;
    message.header.destination_prefix_length = 32;
    message.header.table = RouteHeader::RT_TABLE_MAIN;
    message.header.protocol = RouteProtocol::from(ROUTE_PROTOCOL);
    message.header.scope = RouteScope::Link;
    message.header.kind = RouteType::Unicast;
    message.attributes = vec![
        RouteAttribute::Destination(RouteAddress::Inet(route.dst)),
        RouteAttribute::Oif(route.oif),
        RouteAttribute::PrefSource(RouteAddress::Inet(route.src)),
    ];
    message
}
