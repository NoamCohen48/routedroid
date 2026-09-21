//! Minimal async Unix SOCK_SEQPACKET (tokio has no native type for it).
//! Message boundaries are preserved by the kernel; a datagram larger than
//! the receive buffer is truncated, which `recv` reports as an error.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::unix::AsyncFd;

pub struct SeqPacket {
    fd: AsyncFd<OwnedFd>,
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor we own.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn sockaddr(path: &Path) -> io::Result<(libc::sockaddr_un, libc::socklen_t)> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: sockaddr_un is plain data.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "socket path too long"));
    }
    for (d, s) in addr.sun_path.iter_mut().zip(bytes) {
        *d = *s as libc::c_char;
    }
    let len = std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1;
    Ok((addr, len as libc::socklen_t))
}

impl SeqPacket {
    /// Wrap an already-connected SOCK_SEQPACKET descriptor (e.g. from accept).
    pub fn from_owned(fd: OwnedFd) -> Result<Self> {
        set_nonblocking(fd.as_raw_fd())?;
        Ok(Self { fd: AsyncFd::new(fd)? })
    }

    pub async fn connect(path: &Path) -> Result<Self> {
        // SAFETY: plain socket creation.
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error()).context("socket");
        }
        // SAFETY: raw is a fresh owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let (addr, len) = sockaddr(path)?;
        // SAFETY: addr/len describe a valid sockaddr_un.
        if unsafe { libc::connect(fd.as_raw_fd(), &addr as *const _ as *const libc::sockaddr, len) } < 0 {
            return Err(io::Error::last_os_error()).with_context(|| format!("connect {}", path.display()));
        }
        Self::from_owned(fd)
    }

    pub async fn send(&self, msg: &[u8]) -> io::Result<()> {
        loop {
            let mut g = self.fd.writable().await?;
            match g.try_io(|inner| {
                // SAFETY: msg is a valid slice for the call.
                let n = unsafe { libc::send(inner.as_raw_fd(), msg.as_ptr().cast(), msg.len(), libc::MSG_NOSIGNAL) };
                if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
            }) {
                Ok(Ok(n)) if n == msg.len() => return Ok(()),
                Ok(Ok(n)) => return Err(io::Error::other(format!("short seqpacket send {n}/{}", msg.len()))),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_) => continue,
            }
        }
    }

    /// Receive one datagram; `Ok(0)` means the peer closed.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut g = self.fd.readable().await?;
            match g.try_io(|inner| {
                // SAFETY: buf is a valid writable slice for the call.
                let n = unsafe { libc::recv(inner.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), libc::MSG_TRUNC) };
                if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
            }) {
                Ok(Ok(n)) if n > buf.len() => return Err(io::Error::other(format!("datagram of {n} bytes truncated"))),
                Ok(Ok(n)) => return Ok(n),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_) => continue,
            }
        }
    }
}

/// Listening socket: from systemd (`LISTEN_FDS`) or bound here.
pub struct Listener {
    fd: AsyncFd<OwnedFd>,
}

impl Listener {
    pub fn from_systemd() -> Result<Option<Self>> {
        let pid: u32 = match std::env::var("LISTEN_PID") { Ok(p) => p.parse().unwrap_or(0), Err(_) => return Ok(None) };
        if pid != std::process::id() {
            return Ok(None);
        }
        let n: u32 = std::env::var("LISTEN_FDS").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        if n != 1 {
            anyhow::bail!("expected exactly one socket from systemd, got {n}");
        }
        // SAFETY: fd 3 is handed to us by systemd and owned by this process.
        let fd = unsafe { OwnedFd::from_raw_fd(3) };
        set_nonblocking(3)?;
        Ok(Some(Self { fd: AsyncFd::new(fd)? }))
    }

    pub fn bind(path: &Path) -> Result<Self> {
        let _ = std::fs::remove_file(path);
        // SAFETY: plain socket creation.
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error()).context("socket");
        }
        // SAFETY: raw is a fresh owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let (addr, len) = sockaddr(path)?;
        // SAFETY: addr/len describe a valid sockaddr_un.
        if unsafe { libc::bind(fd.as_raw_fd(), &addr as *const _ as *const libc::sockaddr, len) } < 0 {
            return Err(io::Error::last_os_error()).with_context(|| format!("bind {}", path.display()));
        }
        if unsafe { libc::listen(fd.as_raw_fd(), 4) } < 0 {
            return Err(io::Error::last_os_error()).context("listen");
        }
        Ok(Self { fd: AsyncFd::new(fd)? })
    }

    pub async fn accept(&self) -> Result<SeqPacket> {
        loop {
            let mut g = self.fd.readable().await?;
            match g.try_io(|inner| {
                // SAFETY: accept4 with null address is valid.
                let raw = unsafe { libc::accept4(inner.as_raw_fd(), std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC) };
                if raw < 0 { Err(io::Error::last_os_error()) } else { Ok(raw) }
            }) {
                // SAFETY: fresh descriptor from accept4.
                Ok(Ok(raw)) => return SeqPacket::from_owned(unsafe { OwnedFd::from_raw_fd(raw) }),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e).context("accept"),
                Err(_) => continue,
            }
        }
    }

    /// Peer credentials of an accepted connection (SO_PEERCRED).
    pub fn peer_uid(sock: &SeqPacket) -> io::Result<u32> {
        // SAFETY: ucred is plain data; getsockopt fills it.
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(sock.fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len)
        };
        if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(cred.uid) }
    }
}
