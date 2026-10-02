//! Async Unix SOCK_SEQPACKET (tokio has no native type for it). The kernel
//! keeps message boundaries, so one `send` is one [`Datagram`](crate::Datagram).

mod activation;
mod listener;

use std::io::{self, IoSlice};
use std::path::Path;

use rustix::net::RecvFlags;
use serde::Serialize;
use socket2::{Domain, SockAddr, Socket, Type};
use tokio::io::unix::AsyncFd;

pub use activation::{Activated, Activation};
pub use listener::Listener;

use crate::MAX_PACKET;
use crate::datagram::{KIND_PACKET, encode_control};

pub struct SeqPacket {
    socket: AsyncFd<Socket>,
}

impl SeqPacket {
    /// Wrap a connected socket (from `accept`, `connect` or systemd).
    fn from_socket(socket: Socket) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: AsyncFd::new(socket)?,
        })
    }

    /// Connect in blocking mode, so a full backlog waits instead of failing
    /// with EAGAIN, then switch to non-blocking I/O.
    pub fn connect(path: &Path) -> io::Result<Self> {
        // `Socket::new` sets CLOEXEC on Linux.
        let socket = Socket::new(Domain::UNIX, Type::SEQPACKET, None)?;
        socket.connect(&SockAddr::unix(path)?)?;
        Self::from_socket(socket)
    }

    pub async fn send_control<M: Serialize>(&self, message: &M) -> io::Result<()> {
        self.send(&[IoSlice::new(&encode_control(message))]).await
    }

    /// Send one IPv4 packet; the kind byte and the packet go out as one
    /// datagram without copying the packet.
    pub async fn send_packet(&self, packet: &[u8]) -> io::Result<()> {
        if packet.len() > MAX_PACKET {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}-byte packet", packet.len()),
            ));
        }
        self.send(&[IoSlice::new(&[KIND_PACKET]), IoSlice::new(packet)])
            .await
    }

    /// Send one IPv4 packet only if the peer has room now; `false` means its
    /// queue is full and nothing was sent. A packet relay drops rather than
    /// waits, so one slow direction never stalls the other.
    pub fn try_send_packet(&self, packet: &[u8]) -> io::Result<bool> {
        if packet.len() > MAX_PACKET {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}-byte packet", packet.len()),
            ));
        }
        let parts = [IoSlice::new(&[KIND_PACKET]), IoSlice::new(packet)];
        let total = 1 + packet.len();
        loop {
            match self.socket.get_ref().send_vectored(&parts) {
                Ok(sent) if sent == total => return Ok(true),
                Ok(sent) => {
                    return Err(io::Error::other(format!(
                        "short seqpacket send {sent}/{total}"
                    )));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) => return Err(e),
            }
        }
    }

    async fn send(&self, parts: &[IoSlice<'_>]) -> io::Result<()> {
        let total: usize = parts.iter().map(|part| part.len()).sum();
        loop {
            let mut guard = self.socket.writable().await?;
            match guard.try_io(|inner| inner.get_ref().send_vectored(parts)) {
                Ok(Ok(sent)) if sent == total => return Ok(()),
                Ok(Ok(sent)) => {
                    return Err(io::Error::other(format!(
                        "short seqpacket send {sent}/{total}"
                    )));
                }
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }

    /// Receive one datagram into `buf`; `None` means the peer closed. A
    /// datagram longer than `buf` is an error (`MSG_TRUNC` reports its real
    /// length), never a silently shortened message.
    pub async fn recv<'b>(&self, buf: &'b mut [u8]) -> io::Result<Option<&'b [u8]>> {
        let len = loop {
            let mut guard = self.socket.readable().await?;
            match guard.try_io(|inner| {
                Ok(rustix::net::recv(
                    inner.get_ref(),
                    &mut *buf,
                    RecvFlags::TRUNC,
                )?)
            }) {
                Ok(Ok((_, len))) => break len,
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        };
        if len > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{len}-byte datagram truncated"),
            ));
        }
        Ok((len > 0).then(|| &buf[..len]))
    }

    /// Peer credentials (SO_PEERCRED) of the connected socket.
    pub fn peer_uid(&self) -> io::Result<u32> {
        Ok(
            rustix::net::sockopt::socket_peercred(self.socket.get_ref())?
                .uid
                .as_raw(),
        )
    }
}

#[cfg(test)]
mod tests;
