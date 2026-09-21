//! Minimal async Unix SOCK_SEQPACKET (tokio has no native type for it).
//! Message boundaries are preserved by the kernel. Sockets come from
//! `socket2`; the only raw descriptor handled here is the one systemd
//! passes to the helper.

use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;

use anyhow::{Context, Result};
use socket2::{Domain, SockAddr, Socket, Type};
use tokio::io::unix::AsyncFd;

pub struct SeqPacket {
    socket: AsyncFd<Socket>,
}

fn new_socket() -> io::Result<Socket> {
    // `Socket::new` sets CLOEXEC on Linux.
    let socket = Socket::new(Domain::UNIX, Type::SEQPACKET, None)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

impl SeqPacket {
    /// Wrap an already-connected SOCK_SEQPACKET socket (e.g. from accept).
    pub fn from_socket(socket: Socket) -> Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self { socket: AsyncFd::new(socket)? })
    }

    pub async fn connect(path: &Path) -> Result<Self> {
        let socket = new_socket().context("socket")?;
        socket.connect(&SockAddr::unix(path)?).with_context(|| format!("connect {}", path.display()))?;
        Self::from_socket(socket)
    }

    pub async fn send(&self, msg: &[u8]) -> io::Result<()> {
        loop {
            let mut guard = self.socket.writable().await?;
            match guard.try_io(|inner| inner.get_ref().write(msg)) {
                Ok(Ok(n)) if n == msg.len() => return Ok(()),
                Ok(Ok(n)) => return Err(io::Error::other(format!("short seqpacket send {n}/{}", msg.len()))),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_) => continue,
            }
        }
    }

    /// Receive one datagram; `Ok(0)` means the peer closed. A datagram that
    /// fills `buf` completely was (or may have been) truncated and is an
    /// error, so callers size `buf` one byte above the largest legal datagram
    /// (`proto::RECV_BUF`).
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.socket.readable().await?;
            match guard.try_io(|inner| inner.get_ref().read(buf)) {
                Ok(Ok(n)) if n == buf.len() => {
                    return Err(io::Error::other(format!("datagram of {n}+ bytes truncated")));
                }
                Ok(Ok(n)) => return Ok(n),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_) => continue,
            }
        }
    }

    /// Peer credentials (SO_PEERCRED) of a connected socket.
    pub fn peer_uid(&self) -> io::Result<u32> {
        Ok(rustix::net::sockopt::socket_peercred(self.socket.get_ref())?.uid.as_raw())
    }
}

/// Listening socket: from systemd (`LISTEN_FDS`) or bound here.
pub struct Listener {
    socket: AsyncFd<Socket>,
}

impl Listener {
    pub fn from_systemd() -> Result<Option<Self>> {
        let pid: u32 = match std::env::var("LISTEN_PID") {
            Ok(p) => p.parse().unwrap_or(0),
            Err(_) => return Ok(None),
        };
        if pid != std::process::id() {
            return Ok(None);
        }
        let n: u32 = std::env::var("LISTEN_FDS").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        if n != 1 {
            anyhow::bail!("expected exactly one socket from systemd, got {n}");
        }
        // SAFETY: fd 3 is handed to us by systemd (sd_listen_fds convention)
        // and is owned by this process; nothing else wraps it.
        let socket = Socket::from(unsafe { OwnedFd::from_raw_fd(3) });
        socket.set_nonblocking(true)?;
        Ok(Some(Self { socket: AsyncFd::new(socket)? }))
    }

    pub fn bind(path: &Path) -> Result<Self> {
        let _ = std::fs::remove_file(path);
        let socket = new_socket().context("socket")?;
        socket.bind(&SockAddr::unix(path)?).with_context(|| format!("bind {}", path.display()))?;
        socket.listen(4).context("listen")?;
        Ok(Self { socket: AsyncFd::new(socket)? })
    }

    pub async fn accept(&self) -> Result<SeqPacket> {
        loop {
            let mut guard = self.socket.readable().await?;
            match guard.try_io(|inner| inner.get_ref().accept()) {
                Ok(Ok((socket, _peer))) => return SeqPacket::from_socket(socket),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e).context("accept"),
                Err(_) => continue,
            }
        }
    }
}
