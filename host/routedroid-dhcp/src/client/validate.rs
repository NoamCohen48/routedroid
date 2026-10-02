//! Strict acceptance of a server reply (architecture §6.1): XID, BOOTREPLY,
//! our hardware address, our client identity, and a wanted message type.
//! The `chaddr` is the host's own MAC, shared with the PC's own DHCP
//! client, so an echoed option 61 is what tells our replies from its.

use std::net::Ipv4Addr;
use std::time::Instant;

use anyhow::{Result, anyhow, bail, ensure};
use tracing::{debug, info, warn};

use super::timing::{classful_mask, mask_prefix};
use super::{Bound, Client};
use crate::dhcp::{self, CLIENT_PORT, Message, MessageType, Options};
use crate::lease::{Lease, Schedule, unix_now};
use crate::packet::{self, Mac, fmt_mac};
use crate::sock::RecvMeta;

/// One validated server reply.
#[derive(Debug)]
pub struct Reply {
    pub msg: Message,
    pub opts: Options,
    pub kind: MessageType,
    pub src_mac: Mac,
    pub src_ip: Ipv4Addr,
    pub received: Instant,
}

/// What the current transaction accepts.
#[derive(Debug)]
pub struct Want<'a> {
    pub xid: u32,
    pub kinds: &'a [MessageType],
    /// Our option 61 body.
    pub client_id: &'a [u8],
}

pub fn validate(frame: &[u8], meta: &RecvMeta, want: &Want<'_>, mac: &Mac) -> Option<Reply> {
    let udp = packet::parse_udp(frame, !meta.csum_not_ready)
        .map_err(|e| debug!(error = %e, len = frame.len(), "ignored frame"))
        .ok()?;
    if udp.dst_port != CLIENT_PORT {
        return None;
    }
    let msg = Message::parse(udp.payload)
        .map_err(|e| debug!(error = %e, from = %udp.src_ip, "ignored malformed DHCP payload"))
        .ok()?;
    let xid = format!("{:#010x}", msg.xid);
    if msg.xid != want.xid || msg.op != dhcp::BOOTREPLY {
        debug!(%xid, op = msg.op, "ignored: not a reply to this transaction");
        return None;
    }
    if msg.htype != dhcp::HTYPE_ETHERNET || msg.hlen != 6 || msg.chaddr_mac() != *mac {
        debug!(chaddr = %fmt_mac(&msg.chaddr_mac()), "ignored: chaddr is not ours");
        return None;
    }
    let opts = Options::from_message(&msg);
    if opts
        .client_id
        .as_deref()
        .is_some_and(|echoed| echoed != want.client_id)
    {
        debug!(%xid, "ignored: option 61 names another client");
        return None;
    }
    let Some(kind) = opts.message_type.filter(|k| want.kinds.contains(k)) else {
        debug!(kind = ?opts.message_type, "ignored: no wanted option 53");
        return None;
    };
    if !opts.malformed.is_empty() {
        warn!(options = ?opts.malformed, "reply carries malformed options (ignored individually)");
    }
    info!(
        kind = %kind, %xid,
        from = %format_args!("{} ({})", udp.src_ip, fmt_mac(&udp.src_mac)),
        to = %format_args!("{} ({})", udp.dst_ip, fmt_mac(&udp.dst_mac)),
        yiaddr = %msg.yiaddr, server_id = ?opts.server_id, lease = ?opts.lease_secs,
        csum_verified = !meta.csum_not_ready, vlan = ?meta.vlan,
        "rx"
    );
    Some(Reply {
        msg,
        opts,
        kind,
        src_mac: udp.src_mac,
        src_ip: udp.src_ip,
        received: Instant::now(),
    })
}

impl Client {
    /// The lease an ACK grants, or why it is unusable.
    pub(super) fn lease_from_ack(&self, r: &Reply) -> Result<Bound> {
        let address = r.msg.yiaddr;
        ensure!(
            !(address.is_unspecified()
                || address.is_broadcast()
                || address.is_multicast()
                || address.is_loopback()),
            "ACK yiaddr {address} is not a usable unicast address"
        );
        let lease_secs = r
            .opts
            .lease_secs
            .ok_or_else(|| anyhow!("ACK without lease time (option 51)"))?;
        let subnet_mask = r.opts.subnet_mask.unwrap_or_else(|| {
            let m = classful_mask(address);
            warn!(%address, mask = %m, "ACK without subnet mask (option 1); using classful default");
            m
        });
        let Some(prefix) = mask_prefix(subnet_mask) else {
            bail!("non-contiguous subnet mask {subnet_mask}");
        };
        let server_id = r.opts.server_id.unwrap_or_else(|| {
            warn!(src = %r.src_ip, "ACK without server identifier (option 54); using IP source");
            r.src_ip
        });
        let t1 = r.opts.t1.unwrap_or(lease_secs / 2);
        let t2 = r.opts.t2.unwrap_or(lease_secs - lease_secs / 8);
        let (t1, t2) = if t1 < t2 && t2 <= lease_secs {
            (t1, t2)
        } else {
            warn!(
                t1,
                t2, lease_secs, "inconsistent timers in ACK; using the RFC defaults"
            );
            (lease_secs / 2, lease_secs - lease_secs / 8)
        };
        let lease = Lease {
            iface: self.link.iface().name.clone(),
            client_id: self.identity.client_id.to_string(),
            address,
            prefix,
            subnet_mask,
            router: r.opts.routers.first().copied(),
            dns: r.opts.dns.clone(),
            static_routes: r.opts.classless_routes.clone(),
            server_id,
            server_mac: fmt_mac(&r.src_mac),
            lease_secs,
            t1,
            t2,
            acquired_at: unix_now(),
            vlan_tagged_replies: self.link.vlan_seen(),
        };
        let schedule = Schedule::new(&lease, r.received);
        Ok(Bound::new(lease, schedule))
    }
}
