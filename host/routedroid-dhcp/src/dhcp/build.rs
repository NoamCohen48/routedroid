//! One builder per row of the architecture §6.1 packet table. The caller
//! picks the frame's addresses (broadcast or unicast); these fix the BOOTP
//! fields and options.

use std::net::Ipv4Addr;

use super::{
    BOOTREQUEST, FLAG_BROADCAST, HTYPE_ETHERNET, MAX_MESSAGE_SIZE, Message, MessageType,
    PARAM_REQUEST_LIST, opt,
};
use crate::identity::ClientId;
use crate::packet::Mac;

/// Who we are on the wire: the interface MAC in `chaddr`, and option 61.
#[derive(Debug, Clone)]
pub struct Identity {
    pub mac: Mac,
    pub client_id: ClientId,
}

fn base(id: &Identity, xid: u32, secs: u16, mtype: MessageType, ciaddr: Ipv4Addr) -> Message {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(&id.mac);
    // The broadcast flag only means something while we have no address the
    // server could unicast to (RFC 2131 §4.1).
    let flags = if ciaddr.is_unspecified() {
        FLAG_BROADCAST
    } else {
        0
    };
    Message {
        op: BOOTREQUEST,
        htype: HTYPE_ETHERNET,
        hlen: 6,
        hops: 0,
        xid,
        secs,
        flags,
        ciaddr,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        options: vec![
            (opt::MESSAGE_TYPE, vec![mtype.as_u8()]),
            (opt::CLIENT_ID, id.client_id.option()),
        ],
    }
}

fn with(mut m: Message, extra: &[(u8, Ipv4Addr)], tail: bool) -> Message {
    for (code, addr) in extra {
        m.options.push((*code, addr.octets().to_vec()));
    }
    if tail {
        m.options
            .push((opt::PARAM_REQUEST, PARAM_REQUEST_LIST.to_vec()));
        let size = MAX_MESSAGE_SIZE.to_be_bytes().to_vec();
        m.options.push((opt::MAX_MESSAGE_SIZE, size));
    }
    m
}

const NONE: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

/// DISCOVER: broadcast, ciaddr 0, option 61, broadcast flag.
pub fn discover(id: &Identity, xid: u32, secs: u16) -> Message {
    with(base(id, xid, secs, MessageType::Discover, NONE), &[], true)
}

/// SELECTING REQUEST: broadcast, requested address (50) and server id (54).
pub fn request_selecting(
    id: &Identity,
    xid: u32,
    secs: u16,
    requested: Ipv4Addr,
    server: Ipv4Addr,
) -> Message {
    let m = base(id, xid, secs, MessageType::Request, NONE);
    with(
        m,
        &[(opt::REQUESTED_IP, requested), (opt::SERVER_ID, server)],
        true,
    )
}

/// INIT-REBOOT REQUEST: broadcast, requested address (50), no server id.
pub fn request_init_reboot(id: &Identity, xid: u32, secs: u16, requested: Ipv4Addr) -> Message {
    let m = base(id, xid, secs, MessageType::Request, NONE);
    with(m, &[(opt::REQUESTED_IP, requested)], true)
}

/// RENEW / REBIND REQUEST: ciaddr = lease, no option 50 or 54. The caller
/// decides unicast (RENEW) or broadcast (REBIND) at the frame level.
pub fn request_renew(id: &Identity, xid: u32, secs: u16, ciaddr: Ipv4Addr) -> Message {
    with(base(id, xid, secs, MessageType::Request, ciaddr), &[], true)
}

/// DECLINE: broadcast, ciaddr 0, the declined address (50) and its server (54).
pub fn decline(id: &Identity, xid: u32, declined: Ipv4Addr, server: Ipv4Addr) -> Message {
    let mut m = base(id, xid, 0, MessageType::Decline, NONE);
    m.flags = 0;
    with(
        m,
        &[(opt::REQUESTED_IP, declined), (opt::SERVER_ID, server)],
        false,
    )
}

/// RELEASE: ciaddr = lease, server id (54). No reply is expected.
pub fn release(id: &Identity, xid: u32, ciaddr: Ipv4Addr, server: Ipv4Addr) -> Message {
    let m = base(id, xid, 0, MessageType::Release, ciaddr);
    with(m, &[(opt::SERVER_ID, server)], false)
}
