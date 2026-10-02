//! `AF_PACKET` socket bound to one interface index, with the BPF filter
//! attached before bind (so nothing unfiltered is ever queued) and
//! `PACKET_AUXDATA` on. Needs `CAP_NET_RAW`.

use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use anyhow::{Context, Result, bail};
use tokio::io::unix::AsyncFd;

mod bpf;
mod iface;
mod recv;

pub use iface::Iface;
pub use recv::{PACKET_OUTGOING, Received, RecvMeta};

use recv::{PACKET_AUXDATA, recvmsg_once, socklen};

fn setsockopt<T>(fd: &OwnedFd, level: libc::c_int, name: libc::c_int, value: &T) -> io::Result<()> {
    // SAFETY: `value` is a live T for the duration of the call.
    let rc = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            level,
            name,
            (&raw const *value).cast(),
            socklen::<T>(),
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Create, look the interface up, filter, enable auxdata, then bind with
/// `ETH_P_ALL`. Protocol 0 at creation means nothing is queued before bind.
fn open(name: &str, nonblocking: bool) -> Result<(OwnedFd, Iface)> {
    let flags =
        libc::SOCK_RAW | libc::SOCK_CLOEXEC | if nonblocking { libc::SOCK_NONBLOCK } else { 0 };
    // SAFETY: plain socket() call; the result is checked below.
    let raw = unsafe { libc::socket(libc::AF_PACKET, flags, 0) };
    if raw < 0 {
        let e = io::Error::last_os_error();
        if matches!(e.raw_os_error(), Some(libc::EPERM | libc::EACCES)) {
            bail!("socket(AF_PACKET): {e}; this needs CAP_NET_RAW");
        }
        return Err(e).context("socket(AF_PACKET)");
    }
    // SAFETY: freshly created and owned by nobody else.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let iface = iface::lookup(&fd, name)?;
    let mut prog = bpf::FILTER;
    let fprog = libc::sock_fprog {
        len: u16::try_from(prog.len()).expect("a short filter"),
        filter: prog.as_mut_ptr(),
    };
    setsockopt(&fd, libc::SOL_SOCKET, libc::SO_ATTACH_FILTER, &fprog)
        .context("SO_ATTACH_FILTER")?;
    setsockopt(&fd, libc::SOL_PACKET, PACKET_AUXDATA, &1 as &libc::c_int)
        .context("PACKET_AUXDATA")?;
    // SAFETY: sockaddr_ll is a plain C struct; zeroed is a valid value.
    let mut sll: libc::sockaddr_ll = unsafe { mem::zeroed() };
    sll.sll_family = libc::sa_family_t::try_from(libc::AF_PACKET).expect("AF_PACKET fits");
    sll.sll_protocol = u16::try_from(libc::ETH_P_ALL)
        .expect("ETH_P_ALL fits")
        .to_be();
    sll.sll_ifindex = iface.index;
    // SAFETY: sll is an initialised sockaddr_ll of the stated size.
    let rc = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&raw const sll).cast(),
            socklen::<libc::sockaddr_ll>(),
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error()).with_context(|| format!("bind(AF_PACKET, {name})"));
    }
    Ok((fd, iface))
}

fn send_on(fd: &OwnedFd, ifindex: i32, frame: &[u8]) -> io::Result<usize> {
    // SAFETY: sockaddr_ll is a plain C struct; zeroed is a valid value.
    let mut sll: libc::sockaddr_ll = unsafe { mem::zeroed() };
    sll.sll_family = libc::sa_family_t::try_from(libc::AF_PACKET).expect("AF_PACKET fits");
    sll.sll_ifindex = ifindex;
    sll.sll_halen = 6;
    sll.sll_addr[..6].copy_from_slice(&frame[..6]);
    sll.sll_protocol = crate::packet::ethertype(frame).unwrap_or(0).to_be();
    // SAFETY: frame and sll are valid for the duration of the call.
    let n = unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            frame.as_ptr().cast(),
            frame.len(),
            0,
            (&raw const sll).cast(),
            socklen::<libc::sockaddr_ll>(),
        )
    };
    usize::try_from(n).map_err(|_| io::Error::last_os_error())
}

fn whole(sent: usize, frame: &[u8]) -> Result<()> {
    if sent == frame.len() {
        Ok(())
    } else {
        bail!("short send: {sent} of {} bytes", frame.len())
    }
}

pub struct PacketSocket {
    fd: AsyncFd<OwnedFd>,
    pub iface: Iface,
}

impl PacketSocket {
    pub fn open(name: &str) -> Result<Self> {
        let (fd, iface) = open(name, true)?;
        Ok(Self {
            fd: AsyncFd::new(fd)?,
            iface,
        })
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<Received> {
        loop {
            let mut guard = self.fd.readable().await?;
            if let Ok(r) = guard.try_io(|inner| recvmsg_once(inner.as_raw_fd(), buf)) {
                return r.context("recvmsg(AF_PACKET)");
            }
        }
    }

    /// Transmit one complete Ethernet frame on the bound interface.
    pub async fn send(&self, frame: &[u8]) -> Result<()> {
        loop {
            let mut guard = self.fd.writable().await?;
            if let Ok(r) = guard.try_io(|inner| send_on(inner.get_ref(), self.iface.index, frame)) {
                return whole(r.context("sendto(AF_PACKET)")?, frame);
            }
        }
    }
}

/// Open, send `frame(iface)`, close, all blocking: for a RELEASE from a
/// context without a runtime, such as crash recovery.
pub fn send_once(name: &str, frame: impl FnOnce(&Iface) -> Vec<u8>) -> Result<()> {
    let (fd, iface) = open(name, false)?;
    let frame = frame(&iface);
    whole(
        send_on(&fd, iface.index, &frame).context("sendto(AF_PACKET)")?,
        &frame,
    )
}
