//! Links over rtnetlink: look up by name, configure, delete by index.

use anyhow::{Context, Result};
use netlink_packet_route::link::{LinkAttribute, LinkFlags, LinkMessage};
use netlink_packet_route::RouteNetlinkMessage;
use routedroid_helper_ipc::IfName;

use super::netlink;
use crate::kernel::Link;

pub fn find(name: &IfName) -> Result<Option<Link>> {
    let mut request = LinkMessage::default();
    request.attributes.push(LinkAttribute::IfName(name.as_str().to_owned()));
    match netlink::get(RouteNetlinkMessage::GetLink(request)) {
        Ok(RouteNetlinkMessage::NewLink(reply)) => Ok(Some(Link {
            index: reply.header.index,
            alias: reply.attributes.into_iter().find_map(|a| match a {
                LinkAttribute::IfAlias(alias) => Some(alias),
                _ => None,
            }),
        })),
        Ok(other) => anyhow::bail!("unexpected reply to RTM_GETLINK: {other:?}"),
        Err(e) if netlink::is_absent(&e) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("look up link {name}")),
    }
}

/// Set the alias and MTU and bring the link up, in one request.
pub fn configure(index: u32, alias: &str, mtu: u32) -> Result<()> {
    let mut request = LinkMessage::default();
    request.header.index = index;
    request.header.flags = LinkFlags::Up;
    request.header.change_mask = LinkFlags::Up;
    request.attributes.push(LinkAttribute::IfAlias(alias.to_owned()));
    request.attributes.push(LinkAttribute::Mtu(mtu));
    netlink::change(RouteNetlinkMessage::SetLink(request), 0).with_context(|| format!("configure link #{index}"))
}

pub fn delete(index: u32) -> Result<()> {
    let mut request = LinkMessage::default();
    request.header.index = index;
    netlink::change(RouteNetlinkMessage::DelLink(request), 0).with_context(|| format!("delete link #{index}"))
}
