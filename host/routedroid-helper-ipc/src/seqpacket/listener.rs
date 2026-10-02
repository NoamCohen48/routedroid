//! A listening SOCK_SEQPACKET socket: from systemd or bound here.

use std::io;
use std::path::Path;

use socket2::{Domain, SockAddr, Socket, Type};
use tokio::io::unix::AsyncFd;

use super::SeqPacket;

const BACKLOG: i32 = 16;

pub struct Listener {
    socket: AsyncFd<Socket>,
}

impl Listener {
    pub(super) fn from_socket(socket: Socket) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: AsyncFd::new(socket)?,
        })
    }

    /// Bind `path`, replacing a stale socket file left by an earlier run.
    pub fn bind(path: &Path) -> io::Result<Self> {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let socket = Socket::new(Domain::UNIX, Type::SEQPACKET, None)?;
        socket.bind(&SockAddr::unix(path)?)?;
        socket.listen(BACKLOG)?;
        Self::from_socket(socket)
    }

    pub async fn accept(&self) -> io::Result<SeqPacket> {
        loop {
            let mut guard = self.socket.readable().await?;
            match guard.try_io(|inner| inner.get_ref().accept()) {
                Ok(Ok((socket, _peer))) => return SeqPacket::from_socket(socket),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }
}
