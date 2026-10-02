//! IPv4 policy-routing rules over rtnetlink: dump, add exclusively, delete
//! by exact match.

use std::net::IpAddr;

use anyhow::{Context, Result};
use netlink_packet_core::{NLM_F_CREATE, NLM_F_EXCL};
use netlink_packet_route::route::RouteProtocol;
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleMessage};
use netlink_packet_route::{AddressFamily, RouteNetlinkMessage};

use super::netlink;
use crate::kernel::Rule;

pub fn all() -> Result<Vec<Rule>> {
    let mut request = RuleMessage::default();
    request.header.family = AddressFamily::Inet;
    let replies = netlink::dump(RouteNetlinkMessage::GetRule(request)).context("dump rules")?;
    Ok(replies
        .iter()
        .filter_map(|reply| match reply {
            RouteNetlinkMessage::NewRule(message) => Some(parse(message)),
            _ => None,
        })
        .collect())
}

fn parse(message: &RuleMessage) -> Rule {
    let mut rule = Rule {
        priority: 0,
        table: u32::from(message.header.table),
        src: None,
        protocol: 0,
    };
    for attribute in &message.attributes {
        match attribute {
            RuleAttribute::Priority(priority) => rule.priority = *priority,
            RuleAttribute::Table(table) => rule.table = *table,
            RuleAttribute::Source(IpAddr::V4(src)) => {
                rule.src = Some((*src, message.header.src_len));
            }
            RuleAttribute::Protocol(protocol) => rule.protocol = (*protocol).into(),
            _ => {}
        }
    }
    rule
}

pub fn add(rule: &Rule) -> Result<()> {
    netlink::change(
        RouteNetlinkMessage::NewRule(message(rule)),
        NLM_F_CREATE | NLM_F_EXCL,
    )
    .with_context(|| format!("add rule {}", describe(rule)))
}

/// `Ok(false)` when there was no such rule.
pub fn delete(rule: &Rule) -> Result<bool> {
    match netlink::change(RouteNetlinkMessage::DelRule(message(rule)), 0) {
        Ok(()) => Ok(true),
        Err(e) if netlink::is_absent(&e) => Ok(false),
        Err(e) => Err(e).with_context(|| format!("delete rule {}", describe(rule))),
    }
}

fn message(rule: &Rule) -> RuleMessage {
    let mut message = RuleMessage::default();
    message.header.family = AddressFamily::Inet;
    message.header.action = RuleAction::ToTable;
    message.attributes = vec![
        RuleAttribute::Priority(rule.priority),
        RuleAttribute::Table(rule.table),
        RuleAttribute::Protocol(RouteProtocol::from(rule.protocol)),
    ];
    if let Some((src, len)) = rule.src {
        message.header.src_len = len;
        message
            .attributes
            .push(RuleAttribute::Source(IpAddr::V4(src)));
    }
    message
}

fn describe(rule: &Rule) -> String {
    let from = rule
        .src
        .map_or("all".into(), |(src, len)| format!("{src}/{len}"));
    format!("pref {} from {from} lookup {}", rule.priority, rule.table)
}
