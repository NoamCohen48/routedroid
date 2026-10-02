//! A minimal synchronous rtnetlink client. Each call opens its own socket,
//! so there is no shared state between threads or sessions, and a reply can
//! never be confused with another request's. Errors keep the kernel's errno
//! (`io::Error::raw_os_error`) so callers can tell `EEXIST` from `ENODEV`.

use std::io;
use std::time::Duration;

use netlink_packet_core::{
    NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_ACK, NLM_F_DUMP, NLM_F_DUMP_INTR,
    NLM_F_REQUEST,
};
use netlink_packet_route::RouteNetlinkMessage;
use netlink_sys::{protocols::NETLINK_ROUTE, Socket, SocketAddr};
use rustix::net::RecvFlags;

/// A dump the kernel reports as interrupted (the table changed mid-dump) is
/// retried this many times before giving up.
const DUMP_ATTEMPTS: usize = 3;
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const RECV_BUFFER: usize = 64 * 1024;
const SEQUENCE: u32 = 1;

/// Every object of one kind (`RTM_GET*` with `NLM_F_DUMP`).
pub fn dump(request: RouteNetlinkMessage) -> io::Result<Vec<RouteNetlinkMessage>> {
    for _ in 0..DUMP_ATTEMPTS {
        let mut out = Vec::new();
        match exchange(request.clone(), NLM_F_DUMP, |message| out.push(message)) {
            Ok(()) => return Ok(out),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("netlink dump kept being interrupted"))
}

/// One object (`RTM_GET*` without dump); the kernel's "no such" errno is kept.
pub fn get(request: RouteNetlinkMessage) -> io::Result<RouteNetlinkMessage> {
    let mut reply = None;
    exchange(request, NLM_F_ACK, |message| reply = Some(message))?;
    reply.ok_or_else(|| io::Error::other("netlink request acknowledged without a reply"))
}

/// A mutation (`RTM_NEW*`/`RTM_SET*`/`RTM_DEL*`), acknowledged by the kernel.
pub fn change(request: RouteNetlinkMessage, flags: u16) -> io::Result<()> {
    exchange(request, NLM_F_ACK | flags, |_| {})
}

fn exchange(
    request: RouteNetlinkMessage,
    flags: u16,
    mut on_message: impl FnMut(RouteNetlinkMessage),
) -> io::Result<()> {
    let socket = open()?;
    let mut header = NetlinkHeader::default();
    header.flags = NLM_F_REQUEST | flags;
    header.sequence_number = SEQUENCE;
    let mut message = NetlinkMessage::new(header, NetlinkPayload::InnerMessage(request));
    message.finalize();
    let mut bytes = vec![0; message.buffer_len()];
    message.serialize(&mut bytes);
    socket.send(&bytes, 0)?;

    let mut buf = vec![0u8; RECV_BUFFER];
    loop {
        let (_, len) = rustix::net::recv(&socket, &mut buf, RecvFlags::TRUNC)?;
        if len > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{len}-byte netlink reply truncated"),
            ));
        }
        let mut offset = 0;
        while offset < len {
            let reply = NetlinkMessage::<RouteNetlinkMessage>::deserialize(&buf[offset..len])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            let size = reply.header.length as usize;
            if size == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "zero-length netlink message",
                ));
            }
            offset += size.next_multiple_of(4);
            if reply.header.sequence_number != SEQUENCE {
                continue;
            }
            if reply.header.flags & NLM_F_DUMP_INTR != 0 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            match reply.payload {
                NetlinkPayload::InnerMessage(inner) => on_message(inner),
                NetlinkPayload::Error(error) => {
                    return match error.code {
                        None => Ok(()),
                        Some(code) => Err(io::Error::from_raw_os_error(-code.get())),
                    };
                }
                NetlinkPayload::Done(done) if done.code != 0 => {
                    return Err(io::Error::from_raw_os_error(-done.code));
                }
                NetlinkPayload::Done(_) => return Ok(()),
                NetlinkPayload::Overrun(_) => return Err(io::Error::other("netlink overrun")),
                _ => {}
            }
        }
    }
}

fn open() -> io::Result<Socket> {
    let mut socket = Socket::new(NETLINK_ROUTE)?;
    socket.bind_auto()?;
    socket.connect(&SocketAddr::new(0, 0))?;
    rustix::net::sockopt::set_socket_timeout(
        &socket,
        rustix::net::sockopt::Timeout::Recv,
        Some(REPLY_TIMEOUT),
    )?;
    Ok(socket)
}

/// Whether `error` is the kernel saying the object does not exist.
pub fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENODEV | libc::ENOENT | libc::ESRCH)
    )
}
