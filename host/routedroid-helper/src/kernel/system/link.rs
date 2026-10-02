//! Links over rtnetlink: look up by name, list, configure, delete by index.

use anyhow::{Context, Result};
use netlink_packet_route::link::{
    InfoKind, LinkAttribute, LinkFlags, LinkInfo, LinkLayerType, LinkMessage,
};
use netlink_packet_route::RouteNetlinkMessage;
use routedroid_helper_ipc::IfName;

use super::netlink;
use crate::kernel::{Link, LinkKind};

pub fn find(name: &IfName) -> Result<Option<Link>> {
    let mut request = LinkMessage::default();
    request
        .attributes
        .push(LinkAttribute::IfName(name.as_str().to_owned()));
    match netlink::get(RouteNetlinkMessage::GetLink(request)) {
        Ok(RouteNetlinkMessage::NewLink(reply)) => Ok(Some(parse(reply))),
        Ok(other) => anyhow::bail!("unexpected reply to RTM_GETLINK: {other:?}"),
        Err(e) if netlink::is_absent(&e) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("look up link {name}")),
    }
}

pub fn all() -> Result<Vec<Link>> {
    let replies = netlink::dump(RouteNetlinkMessage::GetLink(LinkMessage::default()))
        .context("dump links")?;
    Ok(replies
        .into_iter()
        .filter_map(|reply| match reply {
            RouteNetlinkMessage::NewLink(message) => Some(parse(message)),
            _ => None,
        })
        .collect())
}

fn parse(message: LinkMessage) -> Link {
    let header = &message.header;
    let mut link = Link {
        index: header.index,
        name: String::new(),
        alias: None,
        kind: match header.link_layer_type {
            LinkLayerType::Loopback => LinkKind::Loopback,
            LinkLayerType::Ether => LinkKind::Ethernet,
            LinkLayerType::None => LinkKind::TunTap,
            _ => LinkKind::Other,
        },
        up: header.flags.contains(LinkFlags::Up),
        carrier: header.flags.contains(LinkFlags::LowerUp),
        master: None,
    };
    for attribute in message.attributes {
        match attribute {
            LinkAttribute::IfName(name) => link.name = name,
            LinkAttribute::IfAlias(alias) => link.alias = Some(alias),
            LinkAttribute::Controller(index) => link.master = Some(index),
            // A TAP is Ethernet at the link layer; only its kind tells.
            LinkAttribute::LinkInfo(infos)
                if infos
                    .iter()
                    .any(|info| matches!(info, LinkInfo::Kind(InfoKind::Tun))) =>
            {
                link.kind = LinkKind::TunTap;
            }
            _ => {}
        }
    }
    link
}

/// Set the alias and MTU and bring the link up, in one request.
pub fn configure(index: u32, alias: &str, mtu: u32) -> Result<()> {
    let mut request = LinkMessage::default();
    request.header.index = index;
    request.header.flags = LinkFlags::Up;
    request.header.change_mask = LinkFlags::Up;
    request
        .attributes
        .push(LinkAttribute::IfAlias(alias.to_owned()));
    request.attributes.push(LinkAttribute::Mtu(mtu));
    netlink::change(RouteNetlinkMessage::SetLink(request), 0)
        .with_context(|| format!("configure link #{index}"))
}

pub fn delete(index: u32) -> Result<()> {
    let mut request = LinkMessage::default();
    request.header.index = index;
    netlink::change(RouteNetlinkMessage::DelLink(request), 0)
        .with_context(|| format!("delete link #{index}"))
}
