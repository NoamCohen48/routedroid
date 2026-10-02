//! A phone's egress (see [`Egress`]): its table's routes, then its rule;
//! removed in the opposite order, each piece only if it is still there.

use std::net::Ipv4Addr;

use anyhow::{Context, Result};
use netlink_packet_core::{NLM_F_CREATE, NLM_F_EXCL};
use netlink_packet_route::route::{
    RouteAddress, RouteAttribute, RouteHeader, RouteMessage, RouteProtocol, RouteScope, RouteType,
};
use netlink_packet_route::{AddressFamily, RouteNetlinkMessage};

use super::{netlink, route, rule};
use crate::kernel::{Egress, ROUTE_PROTOCOL, Route};

pub fn add(egress: &Egress, lan_index: u32) -> Result<()> {
    let subnet = message(egress, egress.lan_net, egress.prefix, |m| {
        m.header.scope = RouteScope::Link;
        m.attributes.push(RouteAttribute::Oif(lan_index));
    });
    let mut routes = vec![subnet];
    if let Some(gateway) = egress.gateway {
        routes.push(message(egress, Ipv4Addr::UNSPECIFIED, 0, |m| {
            m.attributes
                .push(RouteAttribute::Gateway(RouteAddress::Inet(gateway)));
            m.attributes.push(RouteAttribute::Oif(lan_index));
        }));
    }
    routes.push(message(egress, Ipv4Addr::UNSPECIFIED, 0, |m| {
        m.header.kind = RouteType::Unreachable;
        m.attributes.push(RouteAttribute::Priority(u32::MAX));
    }));
    for message in routes {
        netlink::change(
            RouteNetlinkMessage::NewRoute(message),
            NLM_F_CREATE | NLM_F_EXCL,
        )
        .with_context(|| format!("add a route to table {}", egress.table))?;
    }
    rule::add(&egress.rule())
}

pub fn delete(egress: &Egress) -> Result<()> {
    rule::delete(&egress.rule())?;
    for owned in route::all()?.iter().filter(|r| egress.owns(r)) {
        let mut message = selector(egress, owned);
        message.header.scope = RouteScope::NoWhere;
        message.header.kind = RouteType::Unspec;
        match netlink::change(RouteNetlinkMessage::DelRoute(message), 0) {
            Ok(()) => {}
            Err(e) if netlink::is_absent(&e) => {}
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "delete {}/{} from table {}",
                        owned.dst, owned.prefix, egress.table
                    )
                });
            }
        }
    }
    Ok(())
}

/// The fields every route of the egress shares, plus `fill`'s.
fn message(
    egress: &Egress,
    dst: Ipv4Addr,
    prefix: u8,
    fill: impl FnOnce(&mut RouteMessage),
) -> RouteMessage {
    let mut message = selector(
        egress,
        &Route {
            table: egress.table,
            dst,
            prefix,
            gateway: None,
            oif: None,
            protocol: ROUTE_PROTOCOL,
        },
    );
    message.header.scope = RouteScope::Universe;
    message.header.kind = RouteType::Unicast;
    fill(&mut message);
    message
}

/// Table (as the attribute: it is past 255), protocol and destination.
fn selector(egress: &Egress, route: &Route) -> RouteMessage {
    let mut message = RouteMessage::default();
    message.header.address_family = AddressFamily::Inet;
    message.header.table = RouteHeader::RT_TABLE_UNSPEC;
    message.header.protocol = RouteProtocol::from(ROUTE_PROTOCOL);
    message.header.destination_prefix_length = route.prefix;
    message.attributes = vec![RouteAttribute::Table(egress.table)];
    if route.prefix > 0 {
        message
            .attributes
            .push(RouteAttribute::Destination(RouteAddress::Inet(route.dst)));
    }
    message
}
